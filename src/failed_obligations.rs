use std::collections::HashMap;

use anyhow::{Result, anyhow};
use lsp_types::TextDocumentPositionParams;
use serde_json::Value;
use uuid::Uuid;

use crate::{lsp_client::LspClient, rust_analyzer_mcp::GoalIndexInputs};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProofTreeData {
    pub goal: String,
    pub result: String,
    pub depth: usize,
    pub candidates: Vec<CandidateData>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CandidateData {
    pub kind: String,
    pub result: String,
    pub impl_header: Option<String>,
    pub nested_goals: Vec<ProofTreeData>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Candidates {
    Count(usize),
    Candidates(Vec<GoalCandidate>),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalTree {
    pub goal: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_index: Option<String>,
    pub candidates: Candidates,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalCandidate {
    pub kind: String,
    pub result: String,
    pub impl_header: Option<String>,
    pub nested_goals: Vec<GoalTree>,
}

#[derive(Default)]
pub struct FailedObligationsState {
    failed_obligations: HashMap<String, GoalTree>,
}

impl FailedObligationsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store_failed_obligations(&mut self, parsed_data: ProofTreeData) -> GoalTree {
        let mut goal_tree = self.add_proof_tree(&parsed_data);
        goal_tree.goal_index = None;
        goal_tree
    }

    pub fn get_failed_obligations(&self, goal_index: &str) -> Option<GoalTree> {
        self.failed_obligations.get(goal_index).cloned()
    }

    fn add_proof_tree(&mut self, proof_tree: &ProofTreeData) -> GoalTree {
        let mut candidates = Vec::with_capacity(proof_tree.candidates.len());
        for candidate in proof_tree.candidates.iter() {
            let mut goals = Vec::with_capacity(candidate.nested_goals.len());
            for nested_goal in candidate.nested_goals.iter() {
                let goal_tree = self.add_proof_tree(nested_goal);
                let goal_index = goal_tree.goal_index.clone();
                if let Some(goal_index) = &goal_index {
                    self.failed_obligations
                        .insert(goal_index.clone(), goal_tree);
                }
                goals.push(GoalTree {
                    goal: nested_goal.goal.clone(),
                    result: nested_goal.result.clone(),
                    goal_index,
                    candidates: Candidates::Count(nested_goal.candidates.len()),
                });
            }
            candidates.push(GoalCandidate {
                kind: candidate.kind.clone(),
                result: candidate.result.clone(),
                impl_header: candidate.impl_header.clone(),
                nested_goals: goals,
            });
        }

        let goal_index = if candidates.len() > 0 {
            Some(Uuid::new_v4().to_string())
        } else {
            None
        };
        GoalTree {
            goal: proof_tree.goal.clone(),
            result: proof_tree.result.clone(),
            goal_index,
            candidates: Candidates::Candidates(candidates),
        }
    }
}

pub async fn handle_failed_obligations(
    client: &LspClient,
    state: &mut FailedObligationsState,
    args: TextDocumentPositionParams,
) -> Result<Vec<GoalTree>> {
    let result = client
        .request(
            "rust-analyzer/getFailedObligations",
            serde_json::to_value(args)?,
        )
        .await?;
    let result = result.as_str().ok_or_else(|| anyhow!("Expected String"))?;
    if result.is_empty() {
        return Ok(vec![]);
    }

    let result: Vec<ProofTreeData> = serde_json::from_str(result)?;
    Ok(result
        .into_iter()
        .map(|d| state.store_failed_obligations(d))
        .collect())
}

pub async fn handle_failed_obligations_goal(
    _lsp: &LspClient,
    state: &mut FailedObligationsState,
    args: GoalIndexInputs,
) -> Result<serde_json::Value> {
    let goal_indices = match &args.goal_index {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => return Err(anyhow!("goal_index must be a string or array of strings")),
    };

    if goal_indices.is_empty() {
        return Err(anyhow!("At least one goal_index is required"));
    }

    let mut results = Vec::new();
    for goal_index in goal_indices {
        match state.get_failed_obligations(&goal_index) {
            Some(goal_tree) => results.push(goal_tree),
            None => {
                return Err(anyhow!(
                    "Invalid goal_index '{}' or expired data",
                    goal_index
                ));
            }
        }
    }

    let response = if results.len() == 1 {
        serde_json::to_value(results.into_iter().next())?
    } else {
        serde_json::to_value(results)?
    };

    dbg!(&response);

    Ok(response)
}
