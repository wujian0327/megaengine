use crate::git::pack;
use crate::storage;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, BufWriter, Write};

/// MCP Server implementation for repository operations
pub struct RepoMcpServer;

impl RepoMcpServer {
    /// Get list of available tools
    pub fn get_tools() -> Vec<Value> {
        vec![
            json!({
                "name": "list_repos",
                "description": "List all repositories with their details and refs",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            }),
            json!({
                "name": "get_repo_details",
                "description": "Get detailed information about a specific repository",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo_id": {
                            "type": "string",
                            "description": "The ID of the repository"
                        }
                    },
                    "required": ["repo_id"]
                }
            }),
        ]
    }

    /// Execute a tool with the given arguments
    pub async fn execute_tool(name: &str, args: Value) -> Result<Value> {
        match name {
            "list_repos" => Self::list_repos().await,
            "get_repo_details" => {
                let repo_id = args
                    .get("repo_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing repo_id parameter"))?;
                Self::get_repo_details(repo_id).await
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
        }
    }

    /// List all repositories
    async fn list_repos() -> Result<Value> {
        match storage::repo_model::list_repos().await {
            Ok(repos) => {
                let repo_list: Vec<Value> = repos
                    .iter()
                    .map(|repo| {
                        let mut repo_info = json!({
                            "repo_id": repo.repo_id,
                            "name": repo.p2p_description.name,
                            "creator": repo.p2p_description.creator,
                            "description": repo.p2p_description.description,
                            "path": repo.path.display().to_string(),
                            "bundle": repo.bundle.display().to_string(),
                            "timestamp": repo.p2p_description.timestamp,
                        });

                        // Extract refs information
                        if !repo.bundle.as_os_str().is_empty() {
                            if let Ok(local_refs) =
                                pack::extract_bundle_refs(&repo.bundle.to_string_lossy())
                            {
                                let refs: Vec<Value> = local_refs
                                    .iter()
                                    .map(|(ref_name, commit)| {
                                        json!({
                                            "name": ref_name,
                                            "commit": commit
                                        })
                                    })
                                    .collect();
                                repo_info["refs"] = Value::Array(refs);
                            }
                        }

                        repo_info
                    })
                    .collect();

                Ok(json!({
                    "status": "success",
                    "repositories": repo_list,
                    "count": repo_list.len()
                }))
            }
            Err(e) => Ok(json!({
                "status": "error",
                "error": e.to_string()
            })),
        }
    }

    /// Get details of a specific repository
    async fn get_repo_details(repo_id: &str) -> Result<Value> {
        match storage::repo_model::load_repo_from_db(repo_id).await {
            Ok(Some(repo)) => {
                let mut repo_info = json!({
                    "repo_id": repo.repo_id,
                    "name": repo.p2p_description.name,
                    "creator": repo.p2p_description.creator,
                    "description": repo.p2p_description.description,
                    "path": repo.path.display().to_string(),
                    "bundle": repo.bundle.display().to_string(),
                    "timestamp": repo.p2p_description.timestamp,
                });

                // Check for updates if this is a local repo
                if !repo.path.as_os_str().is_empty() && repo.path.exists() {
                    if let Ok(current_refs) =
                        crate::git::git_repo::read_repo_refs(repo.path.to_str().unwrap_or(""))
                    {
                        if let Ok(local_refs) =
                            pack::extract_bundle_refs(&repo.bundle.to_string_lossy())
                        {
                            repo_info["has_updates"] = Value::Bool(current_refs != local_refs);

                            let local_ref_list: Vec<Value> = local_refs
                                .iter()
                                .map(|(ref_name, commit)| {
                                    json!({
                                        "name": ref_name,
                                        "commit": commit
                                    })
                                })
                                .collect();
                            repo_info["local_refs"] = Value::Array(local_ref_list);

                            let current_ref_list: Vec<Value> = current_refs
                                .iter()
                                .map(|(ref_name, commit)| {
                                    json!({
                                        "name": ref_name,
                                        "commit": commit
                                    })
                                })
                                .collect();
                            repo_info["current_refs"] = Value::Array(current_ref_list);
                        }
                    }
                }

                Ok(json!({
                    "status": "success",
                    "repository": repo_info
                }))
            }
            Ok(None) => Ok(json!({
                "status": "error",
                "error": format!("Repository {} not found", repo_id)
            })),
            Err(e) => Ok(json!({
                "status": "error",
                "error": e.to_string()
            })),
        }
    }
}

pub async fn start_mcp_server() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    tracing::info!("MCP Repository Server started");

    // Send initialization message
    let init_response = json!({
        "version": "1.0",
        "name": "megaengine-repo-mcp",
        "capabilities": {
            "tools": {}
        }
    });
    writeln!(writer, "{}", init_response.to_string())?;
    writer.flush()?;

    // Main request loop
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        if let Err(e) = handle_mcp_request(&line, &mut writer).await {
            tracing::error!("Error handling MCP request: {}", e);
            let error_response = json!({
                "error": e.to_string()
            });
            writeln!(writer, "{}", error_response.to_string())?;
            writer.flush()?;
        }
        line.clear();
    }

    Ok(())
}

async fn handle_mcp_request(
    request: &str,
    writer: &mut BufWriter<io::StdoutLock<'_>>,
) -> Result<()> {
    use anyhow::anyhow;

    let request_data: Value = serde_json::from_str(request)?;

    match request_data.get("method").and_then(|v| v.as_str()) {
        Some("tools/list") => {
            let tools = RepoMcpServer::get_tools();
            let response = json!({
                "tools": tools
            });
            writeln!(writer, "{}", response.to_string())?;
            writer.flush()?;
        }
        Some("tools/call") => {
            let tool_name = request_data
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing tool name"))?;

            let tool_args = request_data
                .get("params")
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or_else(|| json!({}));

            let result = RepoMcpServer::execute_tool(tool_name, tool_args).await?;

            let response = json!({
                "result": result
            });
            writeln!(writer, "{}", response.to_string())?;
            writer.flush()?;
        }
        Some(method) => {
            return Err(anyhow!("Unknown method: {}", method));
        }
        None => {
            return Err(anyhow!("Missing method field"));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tools() {
        let tools = RepoMcpServer::get_tools();
        assert_eq!(tools.len(), 2);
    }
}
