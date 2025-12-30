# Symposium Rust Analyzer

An ACP (Agent Communication Protocol) proxy that wraps an MCP server to provide rust-analyzer LSP integration.

Part of the [Symposium](https://github.com/symposium-dev) project.

## Overview

This project creates an ACP proxy using symposium-sacp that connects to rust-analyzer LSP and exposes its functionality through MCP tools. It bridges the gap between ACP-based agents and rust-analyzer's Language Server Protocol.

## Architecture

```
ACP Agent <-> ACP Proxy <-> MCP Server <-> LSP Bridge <-> rust-analyzer LSP
```

- **ACP Proxy**: The main proxy component that implements the ACP protocol
- **MCP Server**: Exposes rust-analyzer functionality as MCP tools
- **LSP Bridge**: Manages communication with the rust-analyzer LSP server
- **rust-analyzer**: The actual Rust language server

## Available Tools

The proxy exposes the following rust-analyzer tools:

- `rust_analyzer_hover` - Get hover information for symbols
- `rust_analyzer_definition` - Go to definition
- `rust_analyzer_references` - Find all references
- `rust_analyzer_completion` - Get code completions
- `rust_analyzer_symbols` - Get document symbols
- `rust_analyzer_format` - Format documents
- `rust_analyzer_code_actions` - Get available code actions
- `rust_analyzer_set_workspace` - Set workspace root
- `rust_analyzer_diagnostics` - Get file diagnostics
- `rust_analyzer_workspace_diagnostics` - Get workspace diagnostics
- `rust_analyzer_failed_obligations` - Get failed trait obligations (rust-analyzer specific)
- `rust_analyzer_failed_obligations_goal` - Explore nested goals (rust-analyzer specific)

## Requirements

- rust-analyzer must be installed and available in PATH

