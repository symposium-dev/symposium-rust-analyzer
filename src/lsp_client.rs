use anyhow::{Result, anyhow};
use lsp_types::*;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::error;

#[derive(Debug)]
pub struct LspClient {
    child: Child,
    request_tx: mpsc::UnboundedSender<LspMessage>,
    next_id: std::sync::atomic::AtomicU64,
}

enum LspMessage {
    Request(LspRequest),
    Notification(LspNotification),
}

struct LspRequest {
    id: u64,
    method: String,
    params: Value,
    response_tx: oneshot::Sender<Result<Value>>,
}

pub struct LspNotification {
    method: String,
    params: Option<serde_json::Value>,
}

impl LspClient {
    pub async fn new(command: &str, args: &[&str], root_uri: Uri) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdout"))?;

        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);

                let mut string = String::new();
                while let Ok(_) = reader.read_line(&mut string).await {
                    if string.is_empty() {
                        break;
                    }
                    eprint!("[rust-analyzer]: {}", string);
                    string.clear();
                }
            });
        }

        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let pending_requests = std::sync::Arc::new(Mutex::new(HashMap::<
            u64,
            oneshot::Sender<Result<Value>>,
        >::new()));

        // Start I/O tasks
        tokio::spawn(Self::write_task(
            stdin,
            request_rx,
            pending_requests.clone(),
        ));
        tokio::spawn(Self::read_task(stdout, pending_requests));

        let client = Self {
            child,
            request_tx,
            next_id: std::sync::atomic::AtomicU64::new(1),
        };

        // Initialize
        client.initialize(root_uri).await?;

        Ok(client)
    }

    async fn write_task(
        mut stdin: tokio::process::ChildStdin,
        mut request_rx: mpsc::UnboundedReceiver<LspMessage>,
        pending_requests: std::sync::Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
    ) {
        while let Some(req) = request_rx.recv().await {
            let (id, method, params) = match req {
                LspMessage::Request(req) => {
                    // Store the response channel
                    pending_requests
                        .lock()
                        .await
                        .insert(req.id, req.response_tx);
                    (Some(req.id), req.method, Some(req.params))
                }
                LspMessage::Notification(not) => (None, not.method, not.params),
            };

            let mut message = serde_json::Map::new();
            message.insert(
                "jsonrpc".to_string(),
                serde_json::Value::String("2.0".to_string()),
            );
            message.insert("method".to_string(), serde_json::Value::String(method));
            if let Some(id) = id {
                message.insert("id".to_string(), serde_json::Value::Number(id.into()));
            }
            if let Some(params) = params {
                message.insert("params".to_string(), params);
            }

            let content = serde_json::to_string(&message).unwrap();
            let header = format!("Content-Length: {}\r\n\r\n", content.len());

            if let Err(e) = stdin.write_all(header.as_bytes()).await {
                error!("Failed to write header: {}", e);
                if let Some(id) = id
                    && let Some(tx) = pending_requests.lock().await.remove(&id)
                {
                    let _ = tx.send(Err(anyhow!("Failed to write header: {}", e)));
                }
                break;
            }
            if let Err(e) = stdin.write_all(content.as_bytes()).await {
                error!("Failed to write content: {}", e);
                if let Some(id) = id
                    && let Some(tx) = pending_requests.lock().await.remove(&id)
                {
                    let _ = tx.send(Err(anyhow!("Failed to write content: {}", e)));
                }
                break;
            }
            tracing::debug!("Sent LSP message ({} bytes): {}", content.len(), content);
        }
    }

    async fn read_task(
        stdout: tokio::process::ChildStdout,
        pending_requests: std::sync::Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
    ) {
        let mut reader = BufReader::new(stdout);
        let mut buffer = String::new();

        loop {
            buffer.clear();
            if reader.read_line(&mut buffer).await.unwrap_or(0) == 0 {
                break;
            }

            if !buffer.starts_with("Content-Length:") {
                continue;
            }

            let length: usize = buffer
                .trim()
                .strip_prefix("Content-Length:")
                .unwrap()
                .trim()
                .parse()
                .unwrap_or(0);

            if length == 0 {
                continue;
            }

            // Skip empty line
            buffer.clear();
            reader.read_line(&mut buffer).await.unwrap_or(0);

            // Read content
            let mut content = vec![0u8; length];
            if tokio::io::AsyncReadExt::read_exact(&mut reader, &mut content)
                .await
                .is_err()
            {
                break;
            }

            let content_str = String::from_utf8_lossy(&content);
            tracing::debug!("Received LSP message ({} bytes): {}", length, content_str);
            if let Ok(message) = serde_json::from_str::<Value>(&content_str) {
                if let Some(id) = message.get("id").and_then(|v| v.as_u64()) {
                    if let Some(tx) = pending_requests.lock().await.remove(&id) {
                        let result = if let Some(error) = message.get("error") {
                            Err(anyhow!("LSP error: {}", error))
                        } else {
                            Ok(message.get("result").cloned().unwrap_or(Value::Null))
                        };
                        let _ = tx.send(result);
                    }
                }
            }
        }
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        // FIXME: store server status and don't send prior to being ready

        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx.send(LspMessage::Request(LspRequest {
            id,
            method: method.to_string(),
            params,
            response_tx,
        }))?;

        response_rx.await?
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        // FIXME: store server status and don't send prior to being ready

        self.request_tx
            .send(LspMessage::Notification(LspNotification {
                method: method.to_string(),
                params,
            }))?;

        Ok(())
    }

    #[allow(deprecated)]
    async fn initialize(&self, root_uri: Uri) -> Result<()> {
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_path: None,
            root_uri: Some(root_uri),
            initialization_options: Some(serde_json::json!({
                "cargo": { "buildScripts": { "enable": true } },
                "checkOnSave": { "enable": true, "command": "check" },
                "diagnostics": { "enable": true },
                "procMacro": { "enable": true }
            })),
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    hover: Some(HoverClientCapabilities {
                        dynamic_registration: Some(false),
                        content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    }),
                    completion: Some(CompletionClientCapabilities::default()),
                    definition: Some(GotoCapability {
                        dynamic_registration: Some(false),
                        link_support: Some(false),
                    }),
                    references: Some(ReferenceClientCapabilities {
                        dynamic_registration: Some(false),
                    }),
                    document_symbol: Some(DocumentSymbolClientCapabilities {
                        dynamic_registration: Some(false),
                        symbol_kind: None,
                        hierarchical_document_symbol_support: Some(true),
                        tag_support: None,
                    }),
                    formatting: Some(DocumentFormattingClientCapabilities {
                        dynamic_registration: Some(false),
                    }),
                    code_action: Some(CodeActionClientCapabilities {
                        dynamic_registration: Some(false),
                        code_action_literal_support: None,
                        is_preferred_support: Some(false),
                        disabled_support: Some(false),
                        data_support: Some(false),
                        resolve_support: None,
                        honors_change_annotations: Some(false),
                    }),
                    publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                        related_information: Some(true),
                        tag_support: None,
                        version_support: Some(false),
                        code_description_support: Some(false),
                        data_support: Some(false),
                    }),
                    ..Default::default()
                }),
                workspace: Some(WorkspaceClientCapabilities {
                    did_change_configuration: Some(DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(false),
                    }),
                    ..Default::default()
                }),
                window: Some(WindowClientCapabilities {
                    work_done_progress: Some(true),
                    show_message: None,
                    show_document: None,
                }),
                experimental: Some(serde_json::json!({
                    "serverStatusNotification": true,
                })),
                ..Default::default()
            },
            trace: Some(TraceValue::Off),
            workspace_folders: None,
            client_info: Some(ClientInfo {
                name: "symposium-rust-analyzer".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            locale: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let _response = self
            .request("initialize", serde_json::to_value(params)?)
            .await?;

        /*
        if let Some(result) = response.get("capabilities") {
            let capabilities: lsp_types::ServerCapabilities = serde_json::from_value(result.clone())?;
            *self.capabilities.write().await = Some(ServerCapabilities::new(capabilities));
        }
        */

        self.notify("initialized", Some(serde_json::json!({})))
            .await?;

        Ok(())
    }

    pub async fn hover(&self, uri: Uri, position: Position) -> Result<Option<Hover>> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let result = self
            .request("textDocument/hover", serde_json::to_value(params)?)
            .await?;
        Ok(serde_json::from_value(result).unwrap_or(None))
    }

    pub async fn goto_definition(
        &self,
        uri: Uri,
        position: Position,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = self
            .request("textDocument/definition", serde_json::to_value(params)?)
            .await?;
        Ok(serde_json::from_value(result).unwrap_or(None))
    }

    pub async fn find_references(
        &self,
        uri: Uri,
        position: Position,
        include_declaration: bool,
    ) -> Result<Option<Vec<Location>>> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration,
            },
        };

        let result = self
            .request("textDocument/references", serde_json::to_value(params)?)
            .await?;
        Ok(serde_json::from_value(result).unwrap_or(None))
    }

    pub async fn completion(
        &self,
        uri: Uri,
        position: Position,
    ) -> Result<Option<CompletionResponse>> {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };

        let result = self
            .request("textDocument/completion", serde_json::to_value(params)?)
            .await?;
        Ok(serde_json::from_value(result).unwrap_or(None))
    }

    pub async fn document_symbols(&self, uri: Uri) -> Result<Option<DocumentSymbolResponse>> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = self
            .request("textDocument/documentSymbol", serde_json::to_value(params)?)
            .await?;
        Ok(serde_json::from_value(result).unwrap_or(None))
    }

    pub async fn format_document(&self, uri: Uri) -> Result<Option<Vec<TextEdit>>> {
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                properties: HashMap::new(),
                trim_trailing_whitespace: Some(true),
                insert_final_newline: Some(true),
                trim_final_newlines: Some(true),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let result = self
            .request("textDocument/formatting", serde_json::to_value(params)?)
            .await?;
        Ok(serde_json::from_value(result).unwrap_or(None))
    }

    pub async fn code_actions(
        &self,
        uri: Uri,
        range: Range,
        context: CodeActionContext,
    ) -> Result<Option<CodeActionResponse>> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range,
            context,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = self
            .request("textDocument/codeAction", serde_json::to_value(params)?)
            .await?;
        Ok(serde_json::from_value(result).unwrap_or(None))
    }

    pub async fn did_open(
        &self,
        uri: Uri,
        language_id: String,
        version: i32,
        text: String,
    ) -> Result<()> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id,
                version,
                text,
            },
        };

        self.notify("textDocument/didOpen", Some(serde_json::to_value(params)?))
            .await?;
        Ok(())
    }

    pub async fn did_change(
        &self,
        uri: Uri,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> Result<()> {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            content_changes: changes,
        };

        self.notify(
            "textDocument/didChange",
            Some(serde_json::to_value(params)?),
        )
        .await?;
        Ok(())
    }

    pub async fn diagnostics(&self, uri: Uri) -> Result<Option<DocumentDiagnosticReport>> {
        let params = DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result = self
            .request("textDocument/diagnostic", serde_json::to_value(params)?)
            .await?;
        Ok(serde_json::from_value(result).unwrap_or(None))
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
