mod tui;

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use owo_colors::OwoColorize;
use shabka_core::assess::{self, AssessConfig, AssessmentResult, IssueCounts};
use shabka_core::config::{
    self, EmbeddingState, GraphConfig, ShabkaConfig, UpdateCheckState, VALID_PROVIDERS,
};
use shabka_core::decay::{self, PruneConfig, PruneResult};
use shabka_core::embedding::EmbeddingService;
use shabka_core::graph;
use shabka_core::history::{EventAction, HistoryLogger, MemoryEvent};
use shabka_core::model::*;
use shabka_core::ranking::{self, RankCandidate, RankingWeights};
use shabka_core::sharing;
use shabka_core::storage::{create_backend, Storage, StorageBackend};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "shabka", about = "Shabka: Shared LLM Memory System", version)]
enum Cli {
    /// Initialize Shabka in the current project
    Init {
        /// Embedding provider to configure (hash, ollama, openai, gemini)
        #[arg(long, default_value = "hash")]
        provider: String,
        /// Only run prerequisite checks, don't create config
        #[arg(long)]
        check: bool,
    },
    /// Search memories
    Search {
        /// Search query
        query: String,
        /// Filter by memory kind (observation, decision, pattern, error, fix, preference, fact, lesson, todo)
        #[arg(short, long)]
        kind: Option<String>,
        /// Maximum number of results
        #[arg(short, long)]
        limit: Option<usize>,
        /// Filter by tags (can be repeated)
        #[arg(short, long)]
        tag: Option<Vec<String>>,
        /// Filter by project name (derived from cwd)
        #[arg(short, long)]
        project: Option<String>,
        /// Output raw JSON instead of table
        #[arg(long)]
        json: bool,
        /// Cap results to fit within a token budget (estimated)
        #[arg(long)]
        token_budget: Option<usize>,
    },
    /// Get a memory's full details by ID
    Get {
        /// Memory ID (full UUID or short 8-char prefix)
        id: String,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Show system status
    Status,
    /// Export memories to JSON
    Export {
        /// Output file path
        #[arg(short, long, default_value = "shabka-export.json")]
        output: String,
        /// Privacy threshold: only export memories at this level or more open (public, team, private)
        #[arg(long, default_value = "private")]
        privacy: String,
        /// Scrub PII (emails, API keys, IPs, file paths) from exported content
        #[arg(long)]
        scrub: bool,
        /// Dry run: show what PII would be found without exporting
        #[arg(long)]
        scrub_report: bool,
    },
    /// Import memories from JSON
    Import {
        /// Input file path
        path: String,
    },
    /// Follow a chain of relations from a memory (debugging narratives, version history)
    Chain {
        /// Starting memory ID
        id: String,
        /// Relation types to follow (caused_by, fixes, supersedes, related, contradicts)
        #[arg(short, long)]
        relation: Option<Vec<String>>,
        /// Maximum traversal depth (default from config, fallback 5)
        #[arg(long)]
        depth: Option<usize>,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Prune stale memories (archive those not accessed in N days)
    Prune {
        /// Days of inactivity before archiving (default from config, fallback 90)
        #[arg(long)]
        days: Option<u64>,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Also decay importance of stale memories
        #[arg(long)]
        decay_importance: bool,
    },
    /// Show audit history for a memory or recent events
    History {
        /// Memory ID to show history for (omit for recent events)
        id: Option<String>,
        /// Maximum number of events to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Assess memory quality and find issues
    Assess {
        /// Check for duplicates (slower — requires embedding comparison)
        #[arg(long)]
        duplicates: bool,
        /// Maximum memories to analyze (default: all)
        #[arg(short, long)]
        limit: Option<usize>,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Run diagnostic checks on the Shabka pipeline
    Doctor,
    /// Consolidate clusters of similar memories into comprehensive summaries (requires LLM)
    Consolidate {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Minimum cluster size to consolidate
        #[arg(long)]
        min_cluster: Option<usize>,
        /// Minimum age in days before a memory is eligible
        #[arg(long)]
        min_age: Option<u64>,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Re-embed all memories with the current embedding provider
    Reembed {
        /// Number of memories to process per batch
        #[arg(long, default_value = "10")]
        batch_size: usize,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Force re-embed all memories, ignoring incremental skip logic
        #[arg(long)]
        force: bool,
    },
    /// Set verification status on a memory (verified, disputed, outdated)
    Verify {
        /// Memory ID (full UUID or short 8-char prefix)
        id: String,
        /// Verification status: verified, disputed, outdated, unverified
        #[arg(long)]
        status: String,
    },
    /// Generate a paste-ready context pack from project memories
    ContextPack {
        /// Search query to find relevant memories (default: all)
        #[arg(default_value = "")]
        query: String,
        /// Token budget for the pack (default 2000)
        #[arg(long, default_value = "2000")]
        tokens: usize,
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
        /// Filter by memory kind
        #[arg(short, long)]
        kind: Option<String>,
        /// Filter by tags (can be repeated)
        #[arg(short, long)]
        tag: Option<Vec<String>>,
        /// Output raw JSON instead of markdown
        #[arg(long)]
        json: bool,
        /// Write output to file instead of stdout
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Delete one or more memories
    Delete {
        /// Memory ID to delete (full UUID or short 8-char prefix)
        id: Option<String>,
        /// Filter by memory kind (observation, decision, pattern, error, fix, preference, fact, lesson, todo)
        #[arg(short, long)]
        kind: Option<String>,
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
        /// Filter by status (active, archived, superseded)
        #[arg(short, long)]
        status: Option<String>,
        /// Required for bulk deletion (when using filters instead of a single ID)
        #[arg(long)]
        confirm: bool,
        /// Output raw JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// List memories with optional filters
    List {
        /// Filter by memory kind (observation, decision, pattern, error, fix, preference, fact, lesson, todo)
        #[arg(short, long)]
        kind: Option<String>,
        /// Filter by status (active, archived, superseded)
        #[arg(short, long)]
        status: Option<String>,
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Output raw JSON instead of table
        #[arg(long)]
        json: bool,
    },
    /// Check database integrity
    Check {
        /// Auto-repair: remove orphaned embeddings and broken relations
        #[arg(long)]
        repair: bool,
    },
    /// Launch interactive TUI for browsing memories
    Tui,
    /// Populate sample memories for demonstration
    Demo {
        /// Remove demo memories instead of creating them
        #[arg(long)]
        clean: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .compact()
        .init();

    let cli = Cli::parse();
    let config = ShabkaConfig::load(Some(&std::env::current_dir()?))
        .unwrap_or_else(|_| ShabkaConfig::default_config());
    let user_id = config::resolve_user_id(&config.sharing);

    let result = run(cli, &config, &user_id).await;
    if let Err(ref err) = result {
        let friendly = format_helix_error(err, &config);
        if friendly != format!("{}", err) {
            eprintln!("{}", friendly);
            std::process::exit(1);
        }
    }
    result
}

async fn run(cli: Cli, config: &ShabkaConfig, user_id: &str) -> Result<()> {
    match cli {
        Cli::Init { provider, check } => cmd_init(&provider, check).await,
        Cli::Search {
            query,
            kind,
            limit,
            tag,
            project,
            json,
            token_budget,
        } => {
            let storage = make_storage(config)?;
            let embedder = EmbeddingService::from_config(&config.embedding)
                .context("failed to create embedding service")?;
            cmd_search(
                &storage,
                &embedder,
                user_id,
                &query,
                kind,
                limit,
                tag,
                project,
                json,
                token_budget,
            )
            .await
        }
        Cli::Get { id, json } => {
            let storage = make_storage(config)?;
            cmd_get(&storage, &id, json).await
        }
        Cli::Status => {
            let storage = make_storage(config)?;
            cmd_status(&storage, config, user_id).await
        }
        Cli::Export {
            output,
            privacy,
            scrub,
            scrub_report,
        } => {
            let storage = make_storage(config)?;
            let scrub_config = if scrub || scrub_report {
                Some(config.scrub.clone())
            } else {
                None
            };
            cmd_export(
                &storage,
                &output,
                &privacy,
                scrub_config.as_ref(),
                scrub_report,
            )
            .await
        }
        Cli::Import { path } => {
            let storage = make_storage(config)?;
            let embedder = EmbeddingService::from_config(&config.embedding)
                .context("failed to create embedding service")?;
            let history = HistoryLogger::new(config.history.enabled);
            cmd_import(&storage, &embedder, user_id, &path, &history).await
        }
        Cli::Chain {
            id,
            relation,
            depth,
            json,
        } => {
            let storage = make_storage(config)?;
            let depth = depth.unwrap_or(config.graph.max_chain_depth);
            cmd_chain(&storage, &id, relation, depth, json).await
        }
        Cli::Prune {
            days,
            dry_run,
            decay_importance,
        } => {
            let storage = make_storage(config)?;
            let days = days.unwrap_or(config.graph.stale_days);
            let history = HistoryLogger::new(config.history.enabled);
            cmd_prune(&storage, &history, user_id, days, dry_run, decay_importance).await
        }
        Cli::History { id, limit, json } => {
            let history = HistoryLogger::new(config.history.enabled);
            cmd_history(&history, id, limit, json)
        }
        Cli::Assess {
            duplicates,
            limit,
            json,
        } => {
            let storage = make_storage(config)?;
            let embedder = if duplicates {
                Some(
                    EmbeddingService::from_config(&config.embedding)
                        .context("failed to create embedding service")?,
                )
            } else {
                None
            };
            cmd_assess(
                &storage,
                embedder.as_ref(),
                &config.graph,
                limit,
                duplicates,
                json,
            )
            .await
        }
        Cli::Consolidate {
            dry_run,
            min_cluster,
            min_age,
            json,
        } => {
            let storage = make_storage(config)?;
            let embedder = EmbeddingService::from_config(&config.embedding)
                .context("failed to create embedding service")?;
            let history = HistoryLogger::new(config.history.enabled);
            cmd_consolidate(
                &storage,
                &embedder,
                config,
                user_id,
                &history,
                dry_run,
                min_cluster,
                min_age,
                json,
            )
            .await
        }
        Cli::Doctor => cmd_doctor(config).await,
        Cli::Reembed {
            batch_size,
            dry_run,
            force,
        } => {
            let storage = make_storage(config)?;
            let embedder = EmbeddingService::from_config(&config.embedding)
                .context("failed to create embedding service")?;
            cmd_reembed(&storage, &embedder, batch_size, dry_run, force).await
        }
        Cli::Verify { id, status } => {
            let storage = make_storage(config)?;
            let history = HistoryLogger::new(config.history.enabled);
            cmd_verify(&storage, &history, user_id, &id, &status).await
        }
        Cli::ContextPack {
            query,
            tokens,
            project,
            kind,
            tag,
            json,
            output,
        } => {
            let storage = make_storage(config)?;
            let embedder = EmbeddingService::from_config(&config.embedding)
                .context("failed to create embedding service")?;
            cmd_context_pack(
                &storage, &embedder, user_id, &query, tokens, project, kind, tag, json, output,
            )
            .await
        }
        Cli::Delete {
            id,
            kind,
            project,
            status,
            confirm,
            json,
        } => {
            let storage = make_storage(config)?;
            let history = HistoryLogger::new(config.history.enabled);
            cmd_delete(
                &storage, &history, user_id, id, kind, project, status, confirm, json,
            )
            .await
        }
        Cli::List {
            kind,
            status,
            project,
            limit,
            json,
        } => {
            let storage = make_storage(config)?;
            cmd_list(&storage, kind, status, project, limit, json).await
        }
        Cli::Check { repair } => {
            let storage = make_storage(config)?;
            cmd_check(&storage, repair).await
        }
        Cli::Tui => tui::run_tui(config).await,
        Cli::Demo { clean } => {
            let storage = make_storage(config)?;
            let embedder = EmbeddingService::from_config(&config.embedding)
                .context("failed to create embedding service")?;
            let history = HistoryLogger::new(config.history.enabled);
            cmd_demo(&storage, &embedder, user_id, &history, clean).await
        }
    }
}

fn make_storage(config: &ShabkaConfig) -> Result<Storage> {
    create_backend(config).context("failed to create storage backend")
}

/// Format HelixDB connection errors with a user-friendly message.
fn format_helix_error(err: &anyhow::Error, config: &ShabkaConfig) -> String {
    let msg = format!("{:#}", err);
    let is_connection = msg.contains("connection refused")
        || msg.contains("Connection refused")
        || msg.contains("timed out")
        || msg.contains("connect error")
        || msg.contains("dns error")
        || msg.contains("No connection");
    if is_connection {
        format!(
            "{}\n\n  Cannot connect to HelixDB at {}:{}.\n  Make sure HelixDB is running: {}\n",
            "Error: HelixDB unavailable".red(),
            config.helix.url,
            config.helix.port,
            "just db".cyan()
        )
    } else {
        format!("{}", err)
    }
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

async fn check_provider_prereqs(provider: &str) {
    match provider {
        "ollama" => {
            // Check if Ollama is reachable
            let reachable = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                reqwest::get("http://localhost:11434/api/version")
                    .await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false)
            })
            .await
            .unwrap_or(false);
            if !reachable {
                println!(
                    "  {} Ollama not reachable at localhost:11434",
                    "WARNING:".yellow()
                );
                println!(
                    "          Install and start Ollama, then run: {}",
                    "ollama pull nomic-embed-text".cyan()
                );
            } else {
                println!("  {} Ollama is running", "OK:".green());
            }
        }
        "openai" => {
            if std::env::var("OPENAI_API_KEY").is_err() {
                println!(
                    "  {} OPENAI_API_KEY environment variable not set",
                    "WARNING:".yellow()
                );
                println!(
                    "          Set it with: {}",
                    "export OPENAI_API_KEY=sk-...".cyan()
                );
            } else {
                println!("  {} OPENAI_API_KEY is set", "OK:".green());
            }
        }
        "gemini" => {
            if std::env::var("GEMINI_API_KEY").is_err() {
                println!(
                    "  {} GEMINI_API_KEY environment variable not set",
                    "WARNING:".yellow()
                );
                println!(
                    "          Set it with: {}",
                    "export GEMINI_API_KEY=...".cyan()
                );
            } else {
                println!("  {} GEMINI_API_KEY is set", "OK:".green());
            }
        }
        _ => {}
    }
}

async fn cmd_init(provider: &str, check_only: bool) -> Result<()> {
    // Validate provider name
    if !VALID_PROVIDERS.contains(&provider) {
        anyhow::bail!(
            "unknown provider '{}'. Valid options: {}",
            provider,
            VALID_PROVIDERS.join(", ")
        );
    }

    // Run prerequisite checks
    println!("{}", "Checking prerequisites...".dimmed());
    check_provider_prereqs(provider).await;
    println!();

    if check_only {
        println!("{}", "Check complete (no config created).".dimmed());
        return Ok(());
    }

    let cwd = std::env::current_dir()?;
    let shabka_dir = cwd.join(".shabka");

    if shabka_dir.exists() {
        println!("Shabka already initialized in this project.");
        return Ok(());
    }

    std::fs::create_dir_all(&shabka_dir)?;

    let mut config = ShabkaConfig::default_config();

    // Configure embedding provider
    let (model, note) = match provider {
        "ollama" => (
            "nomic-embed-text".to_string(),
            "# Requires Ollama running locally\n",
        ),
        "openai" => (
            "text-embedding-3-small".to_string(),
            "# Set OPENAI_API_KEY env var\n",
        ),
        "gemini" => (
            "text-embedding-004".to_string(),
            "# Set GEMINI_API_KEY env var\n",
        ),
        _ => (
            "hash-128d".to_string(),
            "# Deterministic hashing, no semantic search (for testing)\n",
        ),
    };
    config.embedding.provider = provider.to_string();
    config.embedding.model = model;

    let toml_str = format!("{}{}", note, toml::to_string_pretty(&config)?);
    std::fs::write(shabka_dir.join("config.toml"), toml_str)?;

    // Add config.local.toml to .gitignore if not already present
    let gitignore_path = cwd.join(".gitignore");
    let entry = ".shabka/config.local.toml";
    if gitignore_path.exists() {
        let contents = std::fs::read_to_string(&gitignore_path)?;
        if !contents.lines().any(|l| l.trim() == entry) {
            let mut appended = contents;
            if !appended.ends_with('\n') {
                appended.push('\n');
            }
            appended.push_str(entry);
            appended.push('\n');
            std::fs::write(&gitignore_path, appended)?;
        }
    } else {
        std::fs::write(&gitignore_path, format!("{entry}\n"))?;
    }

    println!("{}", "Initialized Shabka in .shabka/".green());
    println!("  {}   .shabka/config.toml", "Config:".dimmed());
    println!("  {} {}", "Provider:".dimmed(), provider.cyan());
    println!("  {} {}", "Storage:".dimmed(), "sqlite".cyan());
    println!(
        "  {}",
        "Edit .shabka/config.local.toml for local overrides (gitignored)".dimmed()
    );
    println!();
    println!("{}", "Quick Start:".bold());
    println!(
        "  {} {}",
        "Note:".dimmed(),
        "SQLite is the default storage — no HelixDB needed.".dimmed()
    );
    println!("  1. Run MCP server:  {}", "just mcp".cyan());
    println!(
        "  2. Open dashboard:  {} {}  {}",
        "just web".cyan(),
        "->".dimmed(),
        "http://localhost:37737".cyan()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn cmd_search(
    storage: &Storage,
    embedder: &EmbeddingService,
    user_id: &str,
    query: &str,
    kind: Option<String>,
    limit: Option<usize>,
    tags: Option<Vec<String>>,
    project: Option<String>,
    json: bool,
    token_budget: Option<usize>,
) -> Result<()> {
    let limit = limit.unwrap_or(10);
    let kind_filter: Option<MemoryKind> = match &kind {
        Some(k) => Some(k.parse().map_err(|e: String| anyhow::anyhow!("{}", e))?),
        None => None,
    };
    let tag_filter: Vec<String> = tags.unwrap_or_default();

    // Embed query
    let embedding = embedder
        .embed(query)
        .await
        .context("failed to embed query")?;

    // Fetch candidates (over-fetch to allow post-filtering)
    let mut candidates = storage
        .vector_search(&embedding, limit * 3)
        .await
        .context("vector search failed")?;

    // Filter by privacy
    sharing::filter_search_results(&mut candidates, user_id);

    // Get relation counts for ranking
    let memory_ids: Vec<Uuid> = candidates.iter().map(|(m, _)| m.id).collect();
    let counts = storage
        .count_relations(&memory_ids)
        .await
        .unwrap_or_default();
    let count_map: HashMap<Uuid, usize> = counts.into_iter().collect();

    let contradiction_counts = storage
        .count_contradictions(&memory_ids)
        .await
        .unwrap_or_default();
    let contradiction_map: HashMap<Uuid, usize> = contradiction_counts.into_iter().collect();

    // Build rank candidates, applying kind/tag/project filters
    let rank_candidates: Vec<RankCandidate> = candidates
        .into_iter()
        .filter(|(m, _)| {
            if let Some(ref kf) = kind_filter {
                if m.kind != *kf {
                    return false;
                }
            }
            if !tag_filter.is_empty() && !tag_filter.iter().any(|t| m.tags.contains(t)) {
                return false;
            }
            if let Some(ref p) = project {
                if m.project_id.as_deref() != Some(p.as_str()) {
                    return false;
                }
            }
            true
        })
        .map(|(memory, vector_score)| {
            let kw_score = ranking::keyword_score(query, &memory);
            RankCandidate {
                relation_count: count_map.get(&memory.id).copied().unwrap_or(0),
                keyword_score: kw_score,
                contradiction_count: contradiction_map.get(&memory.id).copied().unwrap_or(0),
                memory,
                vector_score,
            }
        })
        .collect();

    let ranked = ranking::rank(rank_candidates, &RankingWeights::default());
    let results: Vec<MemoryIndex> = ranked
        .into_iter()
        .take(limit)
        .map(|r| MemoryIndex::from((&r.memory, r.score)))
        .collect();

    // Apply token budget if set
    let results = match token_budget {
        Some(budget) => ranking::budget_truncate(results, budget),
        None => results,
    };

    if results.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("{}", "No results found.".dimmed());
        }
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        // Table output
        println!(
            "{:<12} {:<12} {:<6} {}",
            "ID".dimmed(),
            "Kind".dimmed(),
            "Score".dimmed(),
            "Title".dimmed()
        );
        for r in &results {
            let short_id = &r.id.to_string()[..8];
            let score_color = if r.score >= 0.7 {
                format!("{:<6.2}", r.score).green().to_string()
            } else if r.score >= 0.4 {
                format!("{:<6.2}", r.score).yellow().to_string()
            } else {
                format!("{:<6.2}", r.score).red().to_string()
            };
            println!(
                "{:<12} {:<12} {} {}",
                short_id.cyan(),
                r.kind.to_string().magenta(),
                score_color,
                r.title
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// context-pack
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn cmd_context_pack(
    storage: &Storage,
    embedder: &EmbeddingService,
    user_id: &str,
    query: &str,
    token_budget: usize,
    project: Option<String>,
    kind: Option<String>,
    tags: Option<Vec<String>>,
    json: bool,
    output: Option<String>,
) -> Result<()> {
    use shabka_core::context_pack::{build_context_pack, format_context_pack};

    let kind_filter: Option<MemoryKind> = match &kind {
        Some(k) => Some(k.parse().map_err(|e: String| anyhow::anyhow!("{}", e))?),
        None => None,
    };
    let tag_filter: Vec<String> = tags.unwrap_or_default();

    // Wide search for candidates
    let search_query = if query.is_empty() { "*" } else { query };
    let embedding = embedder
        .embed(search_query)
        .await
        .context("failed to embed query")?;

    let mut candidates = storage
        .vector_search(&embedding, 50)
        .await
        .context("vector search failed")?;

    // Filter by privacy
    sharing::filter_search_results(&mut candidates, user_id);

    // Get relation counts for ranking
    let memory_ids: Vec<Uuid> = candidates.iter().map(|(m, _)| m.id).collect();
    let counts = storage
        .count_relations(&memory_ids)
        .await
        .unwrap_or_default();
    let count_map: HashMap<Uuid, usize> = counts.into_iter().collect();

    let contradiction_counts = storage
        .count_contradictions(&memory_ids)
        .await
        .unwrap_or_default();
    let contradiction_map: HashMap<Uuid, usize> = contradiction_counts.into_iter().collect();

    // Build rank candidates, applying filters
    let rank_candidates: Vec<RankCandidate> = candidates
        .into_iter()
        .filter(|(m, _)| {
            if let Some(ref kf) = kind_filter {
                if m.kind != *kf {
                    return false;
                }
            }
            if !tag_filter.is_empty() && !tag_filter.iter().any(|t| m.tags.contains(t)) {
                return false;
            }
            if let Some(ref p) = project {
                if m.project_id.as_deref() != Some(p.as_str()) {
                    return false;
                }
            }
            true
        })
        .map(|(memory, vector_score)| {
            let kw_score = ranking::keyword_score(search_query, &memory);
            RankCandidate {
                relation_count: count_map.get(&memory.id).copied().unwrap_or(0),
                keyword_score: kw_score,
                contradiction_count: contradiction_map.get(&memory.id).copied().unwrap_or(0),
                memory,
                vector_score,
            }
        })
        .collect();

    let ranked = ranking::rank(rank_candidates, &RankingWeights::default());
    let memories: Vec<Memory> = ranked.into_iter().map(|r| r.memory).collect();

    // Build context pack
    let pack = build_context_pack(memories, token_budget, project.clone());

    if pack.memories.is_empty() {
        eprintln!("{}", "No memories fit within the token budget.".dimmed());
        return Ok(());
    }

    // Format output
    let text = if json {
        serde_json::to_string_pretty(&pack)?
    } else {
        format_context_pack(&pack)
    };

    // Write to file or stdout
    match output {
        Some(path) => {
            std::fs::write(&path, &text).with_context(|| format!("failed to write to {path}"))?;
            eprintln!(
                "{} {} ({} memories, ~{} tokens)",
                "Wrote".green(),
                path,
                pack.memories.len(),
                pack.total_tokens,
            );
        }
        None => {
            println!("{text}");
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Resolve a memory ID from a full UUID or short prefix.
async fn resolve_memory_id(storage: &Storage, id: &str) -> Result<Uuid> {
    if id.len() < 32 {
        let entries = storage
            .timeline(&TimelineQuery {
                limit: 10000,
                ..Default::default()
            })
            .await
            .context("failed to fetch timeline")?;
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| e.id.to_string().starts_with(id))
            .collect();
        match matches.len() {
            0 => anyhow::bail!("no memory found matching prefix '{id}'"),
            1 => Ok(matches[0].id),
            n => {
                anyhow::bail!("ambiguous prefix '{id}' matches {n} memories. Use a longer prefix.")
            }
        }
    } else {
        Uuid::parse_str(id).context("invalid memory ID")
    }
}

// ---------------------------------------------------------------------------
// get
// ---------------------------------------------------------------------------

async fn cmd_get(storage: &Storage, id: &str, json: bool) -> Result<()> {
    let memory_id = resolve_memory_id(storage, id).await?;

    let memory = storage
        .get_memory(memory_id)
        .await
        .context("memory not found")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&memory)?);
        return Ok(());
    }

    // Header
    println!("{}", memory.title.bold());
    println!(
        "{} {} {}",
        memory.kind.to_string().magenta(),
        format!("importance: {:.0}%", memory.importance * 100.0).dimmed(),
        memory.status.to_string().dimmed()
    );
    println!();

    // Content
    println!("{}", memory.content);
    println!();

    // Metadata
    println!("{}", "--- Details ---".dimmed());
    println!("  {}  {}", "ID:".dimmed(), memory.id.to_string().cyan());
    println!(
        "  {}  {}",
        "Created:".dimmed(),
        memory.created_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!(
        "  {}  {}",
        "Updated:".dimmed(),
        memory.updated_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!(
        "  {}  {}",
        "Accessed:".dimmed(),
        memory.accessed_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!("  {}  {}", "Privacy:".dimmed(), memory.privacy);
    println!("  {}  {}", "Created by:".dimmed(), memory.created_by);
    if !memory.tags.is_empty() {
        println!("  {}  {}", "Tags:".dimmed(), memory.tags.join(", ").cyan());
    }

    // Compute trust score
    let relations = storage.get_relations(memory_id).await.unwrap_or_default();
    let contradiction_count = relations
        .iter()
        .filter(|r| r.relation_type == RelationType::Contradicts)
        .count();
    let trust = shabka_core::trust::trust_score(&memory, contradiction_count);

    println!("  {}  {}", "Verification:".dimmed(), memory.verification);
    println!("  {}  {:.0}%", "Trust:".dimmed(), trust * 100.0);

    // Relations
    if !relations.is_empty() {
        println!();
        println!(
            "{} ({})",
            "--- Relations ---".dimmed(),
            relations.len().to_string().cyan()
        );
        for r in &relations {
            let other_id = if r.source_id == memory_id {
                r.target_id
            } else {
                r.source_id
            };
            let direction = if r.source_id == memory_id { "->" } else { "<-" };
            let rel_color = match r.relation_type {
                RelationType::Fixes => r.relation_type.to_string().green().to_string(),
                RelationType::CausedBy => r.relation_type.to_string().red().to_string(),
                RelationType::Supersedes => r.relation_type.to_string().yellow().to_string(),
                _ => r.relation_type.to_string().blue().to_string(),
            };
            println!(
                "  {} {} {:.2} {}",
                direction,
                rel_color,
                r.strength,
                other_id.to_string()[..8].to_string().cyan()
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// verify
// ---------------------------------------------------------------------------

async fn cmd_verify(
    storage: &Storage,
    history: &HistoryLogger,
    user_id: &str,
    id_str: &str,
    status_str: &str,
) -> Result<()> {
    let id = resolve_memory_id(storage, id_str).await?;
    let verification: VerificationStatus =
        status_str.parse().map_err(|e: String| anyhow::anyhow!(e))?;

    let old_memory = storage.get_memory(id).await.context("memory not found")?;

    let input = UpdateMemoryInput {
        verification: Some(verification),
        ..Default::default()
    };

    let memory = storage.update_memory(id, &input).await?;

    history.log(
        &MemoryEvent::new(id, EventAction::Updated, user_id.to_string())
            .with_title(&memory.title)
            .with_changes(vec![shabka_core::history::FieldChange {
                field: "verification".to_string(),
                old_value: old_memory.verification.to_string(),
                new_value: verification.to_string(),
            }]),
    );

    println!(
        "{} Memory '{}' marked as {}",
        "✓".green(),
        memory.title.bold(),
        verification.to_string().cyan()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// update check
// ---------------------------------------------------------------------------

/// Check for a newer version on GitHub. Returns `Some(latest)` if an update
/// is available, `None` otherwise. Never errors — all failures are silent.
async fn check_for_update() -> Option<String> {
    let current = env!("CARGO_PKG_VERSION");
    let mut state = UpdateCheckState::load();

    // If cache is fresh, use it
    if !state.is_stale() && !state.latest_version.is_empty() {
        let current_ver = semver::Version::parse(current).ok()?;
        let latest_ver = semver::Version::parse(&state.latest_version).ok()?;
        return if latest_ver > current_ver {
            Some(state.latest_version)
        } else {
            None
        };
    }

    // Fetch from GitHub
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .user_agent(format!("shabka/{current}"))
        .build()
        .ok()?;

    let resp = client
        .get("https://api.github.com/repos/mehdig-dev/shabka/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: serde_json::Value = resp.json().await.ok()?;
    let tag = body["tag_name"].as_str()?;
    let tag_version = tag.strip_prefix('v').unwrap_or(tag);
    let html_url = body["html_url"].as_str().unwrap_or("").to_string();

    // Update cache
    state.latest_version = tag_version.to_string();
    state.last_checked = chrono::Utc::now().to_rfc3339();
    state.release_url = html_url;
    let _ = state.save();

    let current_ver = semver::Version::parse(current).ok()?;
    let latest_ver = semver::Version::parse(tag_version).ok()?;
    if latest_ver > current_ver {
        Some(tag_version.to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

async fn cmd_status(storage: &Storage, config: &ShabkaConfig, user_id: &str) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    println!("{}", format!("Shabka Status v{version}").bold());
    println!("  {}    {}", "Version:".dimmed(), version);
    println!("  {}       {}", "User:".dimmed(), user_id);

    // Schema info (SQLite only)
    if let Some((schema_ver, writer_ver)) = storage.schema_info() {
        let writer = writer_ver
            .map(|v| format!(", last written by {v}"))
            .unwrap_or_default();
        println!("  {}   schema v{schema_ver}{writer}", "Database:".dimmed(),);
    }

    // Check HelixDB connectivity
    let timeline_result = storage
        .timeline(&TimelineQuery {
            limit: 1,
            ..Default::default()
        })
        .await;

    match &timeline_result {
        Ok(_) => println!(
            "  {}    {} ({}:{})",
            "HelixDB:".dimmed(),
            "connected".green(),
            config.helix.url,
            config.helix.port
        ),
        Err(e) => println!(
            "  {}    {} ({}:{}) - {}",
            "HelixDB:".dimmed(),
            "disconnected".red(),
            config.helix.url,
            config.helix.port,
            e
        ),
    }

    // Count memories
    if timeline_result.is_ok() {
        let all = storage
            .timeline(&TimelineQuery {
                limit: 10000,
                ..Default::default()
            })
            .await;
        match all {
            Ok(entries) => println!(
                "  {}   {}",
                "Memories:".dimmed(),
                entries.len().to_string().cyan()
            ),
            Err(_) => println!("  {}   {}", "Memories:".dimmed(), "unknown".yellow()),
        }
    } else {
        println!("  {}   {}", "Memories:".dimmed(), "unknown".yellow());
    }

    // Embedding info
    match EmbeddingService::from_config(&config.embedding) {
        Ok(service) => {
            println!(
                "  {}  {} / {} ({}d)",
                "Embedding:".dimmed(),
                service.provider_name().cyan(),
                service.model_id(),
                service.dimensions()
            );
            if let Some(ref url) = config.embedding.base_url {
                println!("  {}   {}", "Base URL:".dimmed(), url);
            }
            // Check for embedding provider migration
            if let Some(warning) = EmbeddingState::migration_warning(
                service.provider_name(),
                service.model_id(),
                service.dimensions(),
            ) {
                println!();
                println!("  {}", warning.replace('\n', "\n  ").yellow());
            }
        }
        Err(e) => {
            println!(
                "  {}  {} / {} ({})",
                "Embedding:".dimmed(),
                config.embedding.provider,
                config.embedding.model,
                format!("NOT CONFIGURED: {}", e).red()
            );
        }
    }

    let capture_status = if config.capture.enabled {
        "enabled".green().to_string()
    } else {
        "disabled".red().to_string()
    };
    println!(
        "  {}    {} (min_importance: {})",
        "Capture:".dimmed(),
        capture_status,
        config.capture.min_importance
    );
    println!(
        "  {}    {} (default)",
        "Privacy:".dimmed(),
        sharing::parse_default_privacy(&config.privacy)
    );

    let config_path = dirs::config_dir()
        .map(|p| p.join("shabka").join("config.toml"))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("  {}     {}", "Config:".dimmed(), config_path);

    // Check for updates (non-blocking, silent on failure)
    if config.updates.check_for_updates {
        if let Some(latest) = check_for_update().await {
            println!();
            println!(
                "  {} v{} -> cargo install shabka-cli (current: v{})",
                "Update available:".yellow().bold(),
                latest,
                version
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// export
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct ExportData {
    memories: Vec<Memory>,
    relations: Vec<MemoryRelation>,
}

async fn cmd_export(
    storage: &Storage,
    output: &str,
    privacy: &str,
    scrub_config: Option<&shabka_core::scrub::ScrubConfig>,
    scrub_report_only: bool,
) -> Result<()> {
    let threshold: MemoryPrivacy = privacy
        .parse()
        .map_err(|e: String| anyhow::anyhow!("{}", e))?;

    // Fetch all memories via timeline
    let entries = storage
        .timeline(&TimelineQuery {
            limit: 10000,
            ..Default::default()
        })
        .await
        .context("failed to fetch timeline")?;

    if entries.is_empty() {
        println!("No memories to export.");
        return Ok(());
    }

    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();

    // Fetch full memories in batches
    let mut memories = storage
        .get_memories(&ids)
        .await
        .context("failed to fetch memories")?;

    // Filter by privacy threshold
    memories.retain(|m| sharing::should_export(m.privacy, threshold));

    if memories.is_empty() {
        println!("No memories match privacy threshold '{}'.", privacy);
        return Ok(());
    }

    // PII scrub report (--scrub-report)
    if scrub_report_only {
        if let Some(cfg) = scrub_config {
            let mut total_emails = 0;
            let mut total_keys = 0;
            let mut total_ips = 0;
            let mut total_paths = 0;
            let mut flagged = 0;

            for m in &memories {
                let text = format!("{}\n{}", m.title, m.content);
                let report = shabka_core::scrub::analyze(&text, cfg);
                let found = report.emails_found
                    + report.api_keys_found
                    + report.ips_found
                    + report.paths_found;
                if found > 0 {
                    flagged += 1;
                    println!(
                        "  {} — emails:{} keys:{} ips:{} paths:{}",
                        &m.id.to_string()[..8],
                        report.emails_found,
                        report.api_keys_found,
                        report.ips_found,
                        report.paths_found
                    );
                }
                total_emails += report.emails_found;
                total_keys += report.api_keys_found;
                total_ips += report.ips_found;
                total_paths += report.paths_found;
            }

            println!(
                "\nPII scan: {} memories scanned, {} flagged",
                memories.len(),
                flagged
            );
            println!(
                "  Emails: {}, API keys: {}, IPs: {}, Paths: {}",
                total_emails, total_keys, total_ips, total_paths
            );
            println!("No file written (report only). Use --scrub to export with redaction.");
        }
        return Ok(());
    }

    // Apply PII scrubbing if requested
    if let Some(cfg) = scrub_config {
        let mut scrubbed_count = 0;
        for m in &mut memories {
            let orig_title = m.title.clone();
            let orig_content = m.content.clone();
            m.title = shabka_core::scrub::scrub(&m.title, cfg);
            m.content = shabka_core::scrub::scrub(&m.content, cfg);
            if m.title != orig_title || m.content != orig_content {
                scrubbed_count += 1;
            }
        }
        if scrubbed_count > 0 {
            println!("PII scrubbed from {} memories.", scrubbed_count);
        }
    }

    // Fetch all relations (only for exported memories)
    let exported_ids: std::collections::HashSet<Uuid> = memories.iter().map(|m| m.id).collect();
    let mut all_relations = Vec::new();
    for memory in &memories {
        if let Ok(rels) = storage.get_relations(memory.id).await {
            // Only include relations where both ends are in the export
            for r in rels {
                if exported_ids.contains(&r.source_id) && exported_ids.contains(&r.target_id) {
                    all_relations.push(r);
                }
            }
        }
    }

    let export = ExportData {
        memories,
        relations: all_relations,
    };

    let json = serde_json::to_string_pretty(&export)?;
    std::fs::write(output, json)?;

    println!(
        "Exported {} memories and {} relations to {} (privacy: {})",
        export.memories.len(),
        export.relations.len(),
        output,
        privacy
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// import
// ---------------------------------------------------------------------------

async fn cmd_import(
    storage: &Storage,
    embedder: &EmbeddingService,
    user_id: &str,
    path: &str,
    history: &HistoryLogger,
) -> Result<()> {
    if !Path::new(path).exists() {
        anyhow::bail!("file not found: {}", path);
    }

    let json = std::fs::read_to_string(path)?;
    let data: ExportData = serde_json::from_str(&json).context("failed to parse export file")?;

    let mut imported_memories = 0;
    let mut imported_relations = 0;
    let mut skipped_test = 0;

    for memory in &data.memories {
        // Skip test data (integration tests tag titles with [test-...])
        if memory.title.contains("[test-")
            || memory.created_by == "integration-test"
            || memory.project_id.as_deref() == Some("test")
        {
            skipped_test += 1;
            continue;
        }

        // Re-assign created_by to current user on import
        let mut m = memory.clone();
        m.created_by = user_id.to_string();

        let embedding = embedder
            .embed(&m.embedding_text())
            .await
            .context("failed to embed memory")?;
        storage
            .save_memory(&m, Some(&embedding))
            .await
            .context("failed to save memory")?;

        history.log(
            &MemoryEvent::new(m.id, EventAction::Imported, user_id.to_string())
                .with_title(&m.title),
        );
        imported_memories += 1;
    }

    for relation in &data.relations {
        storage
            .add_relation(relation)
            .await
            .context("failed to add relation")?;
        imported_relations += 1;
    }

    if skipped_test > 0 {
        println!("Skipped {skipped_test} test memories");
    }
    println!(
        "Imported {} memories and {} relations from {}",
        imported_memories, imported_relations, path
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// chain
// ---------------------------------------------------------------------------

async fn cmd_chain(
    storage: &Storage,
    id: &str,
    relations: Option<Vec<String>>,
    depth: usize,
    json: bool,
) -> Result<()> {
    let start_id =
        Uuid::parse_str(id).context("invalid memory ID (use full UUID or copy from search)")?;

    // Parse relation types (default: all)
    let relation_types: Vec<RelationType> = match relations {
        Some(rels) => rels
            .iter()
            .map(|s| {
                s.parse::<RelationType>()
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .collect::<Result<Vec<_>>>()?,
        None => vec![
            RelationType::CausedBy,
            RelationType::Fixes,
            RelationType::Supersedes,
            RelationType::Related,
            RelationType::Contradicts,
        ],
    };

    // Get starting memory for display
    let start_memory = storage
        .get_memory(start_id)
        .await
        .context("starting memory not found")?;

    let chain = graph::follow_chain(storage, start_id, &relation_types, Some(depth)).await;

    if chain.is_empty() {
        println!(
            "{}",
            format!("No connected memories found from: {}", start_memory.title).dimmed()
        );
        return Ok(());
    }

    // Fetch full details for chain memories
    let chain_ids: Vec<Uuid> = chain.iter().map(|l| l.memory_id).collect();
    let memories = storage
        .get_memories(&chain_ids)
        .await
        .context("failed to fetch chain memories")?;
    let memory_map: HashMap<Uuid, &Memory> = memories.iter().map(|m| (m.id, m)).collect();

    if json {
        let results: Vec<serde_json::Value> = chain
            .iter()
            .filter_map(|link| {
                memory_map.get(&link.memory_id).map(|memory| {
                    serde_json::json!({
                        "id": memory.id.to_string(),
                        "title": memory.title,
                        "kind": memory.kind.to_string(),
                        "relation_type": link.relation_type.to_string(),
                        "from_id": link.from_id.to_string(),
                        "strength": link.strength,
                        "depth": link.depth,
                    })
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        println!(
            "Chain from: {} ({})",
            start_memory.title.bold(),
            id[..8.min(id.len())].to_string().cyan()
        );
        println!();
        for link in &chain {
            if let Some(memory) = memory_map.get(&link.memory_id) {
                let indent = "  ".repeat(link.depth);
                let short_id = &memory.id.to_string()[..8];
                let rel_color = match link.relation_type {
                    RelationType::Fixes => link.relation_type.to_string().green().to_string(),
                    RelationType::CausedBy => link.relation_type.to_string().red().to_string(),
                    RelationType::Supersedes => link.relation_type.to_string().yellow().to_string(),
                    RelationType::Contradicts => {
                        link.relation_type.to_string().magenta().to_string()
                    }
                    _ => link.relation_type.to_string().blue().to_string(),
                };
                println!(
                    "{}{} --[{:.2}]--> {} {} ({})",
                    indent,
                    rel_color,
                    link.strength,
                    short_id.cyan(),
                    memory.title,
                    memory.kind.to_string().dimmed()
                );
            }
        }
    }

    println!(
        "\n{} connected memories found.",
        chain.len().to_string().cyan()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// prune
// ---------------------------------------------------------------------------

async fn cmd_prune(
    storage: &Storage,
    history: &HistoryLogger,
    user_id: &str,
    days: u64,
    dry_run: bool,
    decay_importance: bool,
) -> Result<()> {
    let config = PruneConfig {
        inactive_days: days,
        decay_importance,
        ..Default::default()
    };

    // Fetch all memories via timeline
    let entries = storage
        .timeline(&TimelineQuery {
            limit: 10000,
            ..Default::default()
        })
        .await
        .context("failed to fetch timeline")?;

    if entries.is_empty() {
        println!("No memories found.");
        return Ok(());
    }

    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
    let memories = storage
        .get_memories(&ids)
        .await
        .context("failed to fetch memories")?;

    let now = chrono::Utc::now();
    let actions = decay::analyze(&memories, &config, now);

    if actions.is_empty() {
        println!(
            "{}",
            format!("No stale memories found (threshold: {} days).", days).dimmed()
        );
        return Ok(());
    }

    println!(
        "Found {} stale memories (inactive > {} days):",
        actions.len().to_string().yellow(),
        days
    );
    for action in &actions {
        let imp_info = if let Some(decayed) = action.decayed_importance {
            format!(
                " importance: {} → {}",
                format!("{:.2}", action.current_importance).dimmed(),
                format!("{:.2}", decayed).yellow()
            )
        } else {
            String::new()
        };
        println!(
            "  {} ({}d inactive){} — {}",
            action.memory_id.to_string()[..8].to_string().cyan(),
            action.days_inactive.to_string().red(),
            imp_info,
            action.title
        );
    }

    if dry_run {
        println!("\n{}", "Dry run — no changes made.".yellow());
        return Ok(());
    }

    let mut result = PruneResult::default();
    for action in &actions {
        let mut update = UpdateMemoryInput {
            status: Some(MemoryStatus::Archived),
            ..Default::default()
        };
        if let Some(decayed) = action.decayed_importance {
            update.importance = Some(decayed);
        }

        match storage.update_memory(action.memory_id, &update).await {
            Ok(_) => {
                result.archived += 1;
                if action.decayed_importance.is_some() {
                    result.importance_decayed += 1;
                }
                history.log(
                    &MemoryEvent::new(action.memory_id, EventAction::Archived, user_id.to_string())
                        .with_title(&action.title),
                );
            }
            Err(e) => {
                eprintln!(
                    "  Error archiving {}: {}",
                    &action.memory_id.to_string()[..8],
                    e
                );
                result.errors += 1;
            }
        }
    }

    println!(
        "\nDone: {} archived, {} importance-decayed, {} errors",
        result.archived.to_string().green(),
        result.importance_decayed.to_string().yellow(),
        if result.errors > 0 {
            result.errors.to_string().red().to_string()
        } else {
            result.errors.to_string()
        }
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// history
// ---------------------------------------------------------------------------

fn cmd_history(
    history: &HistoryLogger,
    id: Option<String>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let events = if let Some(ref id_str) = id {
        let memory_id = Uuid::parse_str(id_str).context("invalid memory ID")?;
        history.history_for(memory_id)
    } else {
        history.recent(limit)
    };

    if events.is_empty() {
        println!("{}", "No history events found.".dimmed());
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&events)?);
    } else {
        println!(
            "{:<20} {:<12} {:<8} {}",
            "Timestamp".dimmed(),
            "Action".dimmed(),
            "ID".dimmed(),
            "Title".dimmed()
        );
        for event in events.iter().take(limit) {
            let short_id = &event.memory_id.to_string()[..8];
            let title = event.memory_title.as_deref().unwrap_or("-");
            let action_str = event.action.to_string();
            let action_colored = match event.action {
                EventAction::Created => action_str.green().to_string(),
                EventAction::Updated => action_str.yellow().to_string(),
                EventAction::Deleted => action_str.red().to_string(),
                EventAction::Archived => action_str.dimmed().to_string(),
                EventAction::Imported => action_str.cyan().to_string(),
                EventAction::Superseded => action_str.yellow().to_string(),
            };
            print!(
                "{:<20} {:<21} {:<8} {}",
                event
                    .timestamp
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
                    .dimmed(),
                action_colored,
                short_id.cyan(),
                title
            );
            if !event.changes.is_empty() {
                let changes: Vec<String> = event
                    .changes
                    .iter()
                    .map(|c| {
                        format!(
                            "{}: {} -> {}",
                            c.field.bold(),
                            c.old_value.dimmed(),
                            c.new_value.green()
                        )
                    })
                    .collect();
                print!("  {}", changes.join(", ").dimmed());
            }
            println!();
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// reembed
// ---------------------------------------------------------------------------

async fn cmd_reembed(
    storage: &Storage,
    embedder: &EmbeddingService,
    batch_size: usize,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    let saved_state = EmbeddingState::load();
    let provider_changed = !saved_state.provider.is_empty()
        && !saved_state.matches(
            embedder.provider_name(),
            embedder.model_id(),
            embedder.dimensions(),
        );

    // Check for migration warning before starting
    if provider_changed {
        if let Some(warning) = EmbeddingState::migration_warning(
            embedder.provider_name(),
            embedder.model_id(),
            embedder.dimensions(),
        ) {
            println!("{}", warning);
            println!();
        }
    }

    // Determine whether to do a full or incremental re-embed
    let full_reembed = force || provider_changed || saved_state.last_reembed_at.is_empty();

    // Fetch all memories via timeline
    let entries = storage
        .timeline(&TimelineQuery {
            limit: 10000,
            ..Default::default()
        })
        .await
        .context("failed to fetch timeline")?;

    let total = entries.len();
    println!(
        "  Provider:   {} / {} ({}d)",
        embedder.provider_name(),
        embedder.model_id(),
        embedder.dimensions()
    );

    if total == 0 {
        println!("Nothing to do.");
        return Ok(());
    }

    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
    let all_memories = storage
        .get_memories(&ids)
        .await
        .context("failed to fetch memories")?;

    // Filter to only memories that need re-embedding
    let (memories, skipped) = if full_reembed {
        (all_memories, 0usize)
    } else {
        // Parse last_reembed_at to compare with memory updated_at
        let cutoff = chrono::DateTime::parse_from_rfc3339(&saved_state.last_reembed_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::DateTime::<chrono::Utc>::MIN_UTC);

        let mut to_embed = Vec::new();
        let mut skip_count = 0usize;
        for m in all_memories {
            if m.updated_at > cutoff {
                to_embed.push(m);
            } else {
                skip_count += 1;
            }
        }
        (to_embed, skip_count)
    };

    let count = memories.len();
    if full_reembed {
        println!("Re-embed {} memories (full)", count);
    } else {
        println!(
            "Re-embed {} memories (skipped {} unchanged)",
            count, skipped
        );
    }

    if count == 0 {
        println!("Nothing to do — all memories are up to date.");
        return Ok(());
    }

    if dry_run {
        println!("  Dry run — no changes made.");
        return Ok(());
    }

    let mut processed = 0usize;
    let mut errors = 0usize;

    for chunk in memories.chunks(batch_size) {
        let texts: Vec<String> = chunk.iter().map(|m| m.embedding_text()).collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

        let embeddings = match embedder.embed_batch(&text_refs).await {
            Ok(embs) => embs,
            Err(e) => {
                // Fallback: try one at a time
                eprintln!("  Batch error ({}), falling back to single-item", e);
                let mut single_embs = Vec::with_capacity(chunk.len());
                for text in &text_refs {
                    match embedder.embed(text).await {
                        Ok(emb) => single_embs.push(emb),
                        Err(e2) => {
                            eprintln!("  Error embedding: {}", e2);
                            errors += 1;
                            single_embs.push(Vec::new());
                        }
                    }
                }
                single_embs
            }
        };

        for (memory, embedding) in chunk.iter().zip(embeddings.iter()) {
            if embedding.is_empty() {
                continue;
            }
            match storage.save_memory(memory, Some(embedding)).await {
                Ok(()) => processed += 1,
                Err(e) => {
                    eprintln!("  Error saving {}: {}", &memory.id.to_string()[..8], e);
                    errors += 1;
                }
            }
        }

        eprint!("\r  Progress: {}/{}", processed + errors, count);
    }

    eprintln!();
    println!("Done: {} re-embedded, {} errors", processed, errors);

    // Update embedding state so future runs know what provider was used
    let mut state = EmbeddingState::from_provider(
        embedder.provider_name(),
        embedder.model_id(),
        embedder.dimensions(),
    );
    state.last_reembed_at = chrono::Utc::now().to_rfc3339();
    if let Err(e) = state.save() {
        eprintln!("Warning: failed to save embedding state: {}", e);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// assess
// ---------------------------------------------------------------------------

async fn cmd_assess(
    storage: &Storage,
    embedder: Option<&EmbeddingService>,
    graph_config: &GraphConfig,
    limit: Option<usize>,
    check_duplicates: bool,
    json: bool,
) -> Result<()> {
    // Fetch all memories via timeline
    let entries = storage
        .timeline(&TimelineQuery {
            limit: limit.unwrap_or(10000),
            ..Default::default()
        })
        .await
        .context("failed to fetch timeline")?;

    if entries.is_empty() {
        println!("No memories to assess.");
        return Ok(());
    }

    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
    let memories = storage
        .get_memories(&ids)
        .await
        .context("failed to fetch memories")?;

    let total = memories.len();

    // Get relation counts for all memories
    let all_ids: Vec<Uuid> = memories.iter().map(|m| m.id).collect();
    let relation_counts = storage.count_relations(&all_ids).await.unwrap_or_default();
    let count_map: HashMap<Uuid, usize> = relation_counts.into_iter().collect();

    let assess_config = AssessConfig {
        stale_days: graph_config.stale_days,
        ..AssessConfig::default()
    };

    // Analyze each memory
    let mut results: Vec<AssessmentResult> = memories
        .iter()
        .filter_map(|m| {
            let rel_count = count_map.get(&m.id).copied().unwrap_or(0);
            let issues = assess::analyze_memory(m, &assess_config, rel_count);
            if issues.is_empty() {
                None
            } else {
                Some(AssessmentResult {
                    memory_id: m.id,
                    title: m.title.clone(),
                    issues,
                })
            }
        })
        .collect();

    // Optional duplicate check
    if check_duplicates {
        if let Some(embedder) = embedder {
            eprint!("Checking for duplicates...");
            let mut dup_count = 0usize;
            for mem in &memories {
                let embedding = match embedder.embed(&mem.embedding_text()).await {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                // Search for similar memories
                let similar = storage
                    .vector_search(&embedding, 5)
                    .await
                    .unwrap_or_default();
                for (other, score) in &similar {
                    if other.id != mem.id && *score > graph_config.similarity_threshold {
                        // Check if we already have this result
                        let existing = results.iter_mut().find(|r| r.memory_id == mem.id);
                        let dup_issue = assess::QualityIssue::PossibleDuplicate {
                            other_id: other.id,
                            other_title: other.title.clone(),
                            similarity: *score,
                        };
                        if let Some(r) = existing {
                            if !r.issues.iter().any(|iss| matches!(iss, assess::QualityIssue::PossibleDuplicate { other_id, .. } if *other_id == other.id)) {
                                r.issues.push(dup_issue);
                                dup_count += 1;
                            }
                        } else {
                            results.push(AssessmentResult {
                                memory_id: mem.id,
                                title: mem.title.clone(),
                                issues: vec![dup_issue],
                            });
                            dup_count += 1;
                        }
                    }
                }
            }
            eprintln!(" found {} potential duplicates.", dup_count);
        }
    }

    // Sort by number of issues (worst first)
    results.sort_by(|a, b| b.issues.len().cmp(&a.issues.len()));

    let score = assess::quality_score(&results, total);
    let counts = IssueCounts::from_results(&results);

    if json {
        let json_results: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let issues: Vec<String> = r.issues.iter().map(|i| i.label().to_string()).collect();
                serde_json::json!({
                    "id": r.memory_id.to_string(),
                    "title": r.title,
                    "issues": issues,
                })
            })
            .collect();
        let output = serde_json::json!({
            "total_memories": total,
            "memories_with_issues": results.len(),
            "score": score,
            "counts": {
                "generic_titles": counts.generic_titles,
                "short_content": counts.short_content,
                "no_tags": counts.no_tags,
                "low_importance": counts.low_importance,
                "stale": counts.stale,
                "orphaned": counts.orphaned,
                "duplicates": counts.duplicates,
                "low_trust": counts.low_trust,
            },
            "issues": json_results,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Pretty print scorecard
    println!("{}", "Memory Quality Assessment".bold());
    println!("{}", "=========================".dimmed());
    println!("Total memories: {}", total.to_string().cyan());
    println!();

    fn pct(count: usize, total: usize) -> String {
        if total == 0 {
            return "0%".to_string();
        }
        format!("{}%", count * 100 / total)
    }

    println!("{}:", "Issues found".bold());
    println!(
        "  {:<20} {:>4}  ({})",
        "Generic titles:",
        counts.generic_titles,
        pct(counts.generic_titles, total)
    );
    println!(
        "  {:<20} {:>4}  ({})",
        "Short content:",
        counts.short_content,
        pct(counts.short_content, total)
    );
    println!(
        "  {:<20} {:>4}  ({})",
        "No tags:",
        counts.no_tags,
        pct(counts.no_tags, total)
    );
    println!(
        "  {:<20} {:>4}  ({})",
        "Low importance:",
        counts.low_importance,
        pct(counts.low_importance, total)
    );
    println!(
        "  {:<20} {:>4}  ({})",
        format!("Stale (>{}d):", graph_config.stale_days),
        counts.stale,
        pct(counts.stale, total)
    );
    println!(
        "  {:<20} {:>4}  ({})",
        "Orphaned:",
        counts.orphaned,
        pct(counts.orphaned, total)
    );
    if check_duplicates {
        println!(
            "  {:<20} {:>4}  ({})",
            "Duplicates:",
            counts.duplicates,
            pct(counts.duplicates, total)
        );
    }
    println!(
        "  {:<20} {:>4}  ({})",
        "Low trust:",
        counts.low_trust,
        pct(counts.low_trust, total)
    );

    // Top issues (up to 10)
    if !results.is_empty() {
        println!();
        println!("{}:", "Top issues".bold());
        for r in results.iter().take(10) {
            let short_id = &r.memory_id.to_string()[..8];
            let labels: Vec<&str> = r.issues.iter().map(|i| i.label()).collect();
            println!(
                "  {} {} — {}",
                format!("[{}]", short_id).cyan(),
                format!("\"{}\"", r.title).dimmed(),
                labels.join(", ").yellow()
            );
        }
    }

    println!();
    let score_colored = if score >= 80 {
        format!("{}/100", score).green().to_string()
    } else if score >= 50 {
        format!("{}/100", score).yellow().to_string()
    } else {
        format!("{}/100", score).red().to_string()
    };
    println!("Overall score: {}", score_colored);

    // Actionable suggestions
    let mut suggestions: Vec<&str> = Vec::new();
    if counts.generic_titles > 0 {
        suggestions.push("Generic titles: review with `shabka get <id>` and update titles");
    }
    if counts.no_tags > 0 {
        suggestions.push("No tags: add tags to improve searchability");
    }
    if counts.stale > 0 {
        suggestions.push("Stale: run `shabka prune` to archive inactive memories");
    }
    if counts.orphaned > 0 {
        suggestions.push("Orphaned: these memories won't appear in relation chains");
    }
    if counts.short_content > 0 {
        suggestions.push("Short content: may lack context for future retrieval");
    }
    if counts.duplicates > 0 {
        suggestions.push("Duplicates: review and consider deleting or merging");
    }
    if counts.low_trust > 0 {
        suggestions
            .push("Low trust: use `shabka verify <id> --status verified` to confirm or update");
    }
    if !suggestions.is_empty() {
        println!();
        println!("{}:", "Suggestions".bold());
        for s in &suggestions {
            println!("  {} {}", "-".dimmed(), s);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

async fn cmd_doctor(config: &ShabkaConfig) -> Result<()> {
    println!("{}", "Shabka Doctor".bold());
    println!("{}", "=============".dimmed());
    println!();

    let mut critical_fail = false;

    // 1. HelixDB connectivity
    let storage = make_storage(config)?;
    let helix_ok = match storage
        .timeline(&TimelineQuery {
            limit: 1,
            ..Default::default()
        })
        .await
    {
        Ok(_) => {
            println!(
                "  {} HelixDB        {}",
                "OK".green(),
                format!("{}:{}", config.helix.url, config.helix.port).dimmed()
            );
            true
        }
        Err(e) => {
            println!(
                "  {} HelixDB        {} ({})",
                "FAIL".red(),
                format!("{}:{}", config.helix.url, config.helix.port).dimmed(),
                format!("{e:#}").red()
            );
            println!(
                "       {} Start HelixDB with: {}",
                "hint:".dimmed(),
                "just db".cyan()
            );
            critical_fail = true;
            false
        }
    };

    // 2. Embedding provider
    match EmbeddingService::from_config(&config.embedding) {
        Ok(service) => {
            // Try a test embed
            match service.embed("shabka doctor test").await {
                Ok(vec) => {
                    println!(
                        "  {} Embedding      {} / {} ({}d, vec_len={})",
                        "OK".green(),
                        service.provider_name().cyan(),
                        service.model_id(),
                        service.dimensions(),
                        vec.len(),
                    );
                }
                Err(e) => {
                    println!(
                        "  {} Embedding      {} / {} — {}",
                        "FAIL".red(),
                        service.provider_name(),
                        service.model_id(),
                        format!("{e:#}").red()
                    );
                    critical_fail = true;
                }
            }
        }
        Err(e) => {
            println!(
                "  {} Embedding      {} — {}",
                "FAIL".red(),
                config.embedding.provider,
                format!("{e}").red()
            );
            critical_fail = true;
        }
    }

    // 3. Dimension compatibility
    match config::check_dimensions(&config.embedding) {
        Ok(()) => {
            let state = EmbeddingState::load();
            if state.provider.is_empty() {
                println!(
                    "  {} Dimensions     {}",
                    "OK".green(),
                    "no prior state (first run)".dimmed()
                );
            } else {
                println!(
                    "  {} Dimensions     {}d matches stored state",
                    "OK".green(),
                    state.dimensions
                );
            }
        }
        Err(msg) => {
            println!("  {} Dimensions     {}", "WARN".yellow(), msg.yellow());
            // Not a critical failure — just a warning
        }
    }

    // 4. Hooks binary
    match which::which("shabka-hooks") {
        Ok(path) => {
            println!(
                "  {} Hooks binary   {}",
                "OK".green(),
                path.display().to_string().dimmed()
            );
        }
        Err(_) => {
            println!(
                "  {} Hooks binary   {}",
                "FAIL".red(),
                "shabka-hooks not found in PATH".red()
            );
            println!(
                "       {} Install with: {}",
                "hint:".dimmed(),
                "just cli-install".cyan()
            );
            critical_fail = true;
        }
    }

    // 5. Session buffers
    let sessions_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("shabka")
        .join("sessions");
    let buffer_count = if sessions_dir.exists() {
        std::fs::read_dir(&sessions_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
                    .count()
            })
            .unwrap_or(0)
    } else {
        0
    };

    if buffer_count == 0 {
        println!(
            "  {} Buffers        {}",
            "OK".green(),
            "no active session buffers".dimmed()
        );
    } else {
        println!(
            "  {} Buffers        {} active session buffer{}",
            "WARN".yellow(),
            buffer_count.to_string().yellow(),
            if buffer_count == 1 { "" } else { "s" }
        );
    }

    // Summary
    println!();
    if critical_fail {
        println!(
            "{}",
            "Some checks failed. Fix the issues above and re-run `shabka doctor`.".red()
        );
        std::process::exit(1);
    } else if !helix_ok {
        println!(
            "{}",
            "All critical checks passed, but some warnings exist.".yellow()
        );
    } else {
        println!("{}", "All checks passed!".green());
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_consolidate(
    storage: &Storage,
    embedder: &EmbeddingService,
    config: &ShabkaConfig,
    user_id: &str,
    history: &HistoryLogger,
    dry_run: bool,
    min_cluster: Option<usize>,
    min_age: Option<u64>,
    json: bool,
) -> Result<()> {
    if !config.llm.enabled {
        anyhow::bail!("Consolidation requires LLM. Enable it in config.toml under [llm].");
    }

    let llm = shabka_core::llm::LlmService::from_config(&config.llm)
        .context("failed to create LLM service")?;

    let mut consolidate_config = config.consolidate.clone();
    if let Some(min) = min_cluster {
        consolidate_config.min_cluster_size = min;
    }
    if let Some(age) = min_age {
        consolidate_config.min_age_days = age;
    }

    if dry_run && !json {
        println!("{}", "Dry run — no changes will be made".yellow());
    }

    let result = shabka_core::consolidate::consolidate(
        storage,
        embedder,
        &llm,
        &consolidate_config,
        user_id,
        history,
        dry_run,
    )
    .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "\n{}\n  Clusters found: {}\n  Clusters consolidated: {}\n  Memories superseded: {}\n  Memories created: {}",
            "Consolidation complete".green().bold(),
            result.clusters_found,
            result.clusters_consolidated,
            result.memories_superseded,
            result.memories_created,
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// delete
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn cmd_delete(
    storage: &Storage,
    history: &HistoryLogger,
    user_id: &str,
    id: Option<String>,
    kind: Option<String>,
    project: Option<String>,
    status: Option<String>,
    confirm: bool,
    json: bool,
) -> Result<()> {
    if let Some(ref id_str) = id {
        // Single delete
        let memory_id = resolve_memory_id(storage, id_str).await?;
        let memory = storage
            .get_memory(memory_id)
            .await
            .context("memory not found")?;
        let title = memory.title.clone();
        let kind_str = memory.kind.to_string();

        storage
            .delete_memory(memory_id)
            .await
            .context("failed to delete memory")?;

        history.log(
            &MemoryEvent::new(memory_id, EventAction::Deleted, user_id.to_string())
                .with_title(&title),
        );

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "deleted": memory_id.to_string(),
                    "title": title,
                    "kind": kind_str,
                })
            );
        } else {
            println!(
                "{} {} ({}) [{}]",
                "Deleted:".red(),
                title,
                memory_id.to_string()[..8].to_string().cyan(),
                kind_str.magenta()
            );
        }
    } else if kind.is_some() || project.is_some() || status.is_some() {
        // Bulk delete with filters
        if !confirm {
            anyhow::bail!(
                "bulk delete requires --confirm flag. Use filters (--kind, --project, --status) to select memories."
            );
        }

        let query = TimelineQuery {
            limit: 10000,
            project_id: project,
            ..Default::default()
        };
        let mut entries = storage
            .timeline(&query)
            .await
            .context("failed to fetch timeline")?;

        // Apply kind filter
        if let Some(ref kind_str) = kind {
            if let Ok(k) = kind_str.parse::<MemoryKind>() {
                entries.retain(|e| e.kind == k);
            } else {
                anyhow::bail!("unknown memory kind: {kind_str}");
            }
        }

        // Apply status filter
        if let Some(ref status_str) = status {
            let st: MemoryStatus = serde_json::from_str(&format!("\"{status_str}\""))
                .map_err(|_| anyhow::anyhow!("unknown status: {status_str}"))?;
            entries.retain(|e| e.status == st);
        }

        if entries.is_empty() {
            if json {
                println!("{}", serde_json::json!({ "deleted": 0 }));
            } else {
                println!("No matching memories found.");
            }
            return Ok(());
        }

        let mut deleted = 0usize;
        for entry in &entries {
            if storage.delete_memory(entry.id).await.is_ok() {
                history.log(
                    &MemoryEvent::new(entry.id, EventAction::Deleted, user_id.to_string())
                        .with_title(&entry.title),
                );
                deleted += 1;
            }
        }

        if json {
            println!("{}", serde_json::json!({ "deleted": deleted }));
        } else {
            println!(
                "{} {} memor{}",
                "Deleted".red(),
                deleted,
                if deleted == 1 { "y" } else { "ies" }
            );
        }
    } else {
        anyhow::bail!(
            "usage: shabka delete <ID> or shabka delete --kind <kind> --confirm\n\
             Provide a memory ID for single delete, or use filters (--kind, --project, --status) with --confirm for bulk delete."
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

async fn cmd_list(
    storage: &Storage,
    kind: Option<String>,
    status: Option<String>,
    project: Option<String>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let kind_filter = kind
        .as_deref()
        .map(|s| {
            s.parse::<MemoryKind>()
                .map_err(|_| anyhow::anyhow!("unknown memory kind: {s}"))
        })
        .transpose()?;

    let status_filter = status
        .as_deref()
        .map(|s| {
            serde_json::from_str::<MemoryStatus>(&format!("\"{s}\""))
                .map_err(|_| anyhow::anyhow!("unknown status: {s}"))
        })
        .transpose()?;

    let query = TimelineQuery {
        limit,
        project_id: project,
        kind: kind_filter,
        status: status_filter,
        ..Default::default()
    };

    let entries = storage
        .timeline(&query)
        .await
        .context("failed to fetch timeline")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    if entries.is_empty() {
        println!("No memories found.");
        return Ok(());
    }

    // Table header
    println!(
        "  {}  {}  {}  {}  {}",
        format!("{:<8}", "ID").dimmed(),
        format!("{:<12}", "Kind").dimmed(),
        format!("{:<5}", "Imp").dimmed(),
        format!("{:<10}", "Date").dimmed(),
        "Title".dimmed(),
    );
    println!("{}", "─".repeat(78).dimmed());

    for entry in &entries {
        let short_id = &entry.id.to_string()[..8];
        let date = entry.created_at.format("%Y-%m-%d");
        let imp = format!("{:.0}%", entry.importance * 100.0);
        println!(
            "  {}  {:<12}  {:<5}  {}  {}",
            short_id.cyan(),
            entry.kind.to_string().magenta(),
            imp.dimmed(),
            date,
            entry.title,
        );
    }

    println!("{}", "─".repeat(78).dimmed());
    println!(
        "  {} memor{}",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" }
    );

    Ok(())
}

const DEMO_PREFIX: &str = "[demo] ";

async fn cmd_demo(
    storage: &Storage,
    embedder: &EmbeddingService,
    user_id: &str,
    history: &HistoryLogger,
    clean: bool,
) -> Result<()> {
    if clean {
        return demo_clean(storage, history, user_id).await;
    }

    // Check if demo data already exists
    let timeline = storage
        .timeline(&TimelineQuery {
            limit: 500,
            ..Default::default()
        })
        .await?;
    if timeline.iter().any(|e| e.title.starts_with(DEMO_PREFIX)) {
        println!(
            "{} Demo data already exists. Use {} to remove it first.",
            "Skipped.".yellow(),
            "shabka demo --clean".cyan()
        );
        return Ok(());
    }

    println!("{}", "Seeding demo memories...".cyan());

    // 12 sample memories across all 9 kinds
    let demos: Vec<(MemoryKind, &str, &str, f32, Vec<&str>)> = vec![
        (
            MemoryKind::Decision,
            "[demo] Use JWT with short-lived access tokens for API auth",
            "Chose JWT over session cookies for the REST API. Access tokens expire in 15 minutes, \
             refresh tokens in 7 days. Stateless validation reduces database load. \
             Trade-off: token revocation requires a deny-list check.",
            0.9,
            vec!["auth", "api", "security"],
        ),
        (
            MemoryKind::Error,
            "[demo] Connection pool exhaustion under load",
            "The API started returning 503s during peak traffic. Root cause: default pool size \
             of 10 connections was too low for 200 concurrent requests. Each request held a \
             connection for ~50ms, creating a bottleneck at the pool checkout.",
            0.8,
            vec!["database", "performance", "production"],
        ),
        (
            MemoryKind::Fix,
            "[demo] Increase pool size and add connection timeout",
            "Fixed the pool exhaustion by increasing max connections to 50 and adding a 5s \
             checkout timeout with a retry. Also added connection pool metrics to the /health \
             endpoint for early warning.",
            0.8,
            vec!["database", "performance"],
        ),
        (
            MemoryKind::Pattern,
            "[demo] Repository pattern for database access",
            "All database access goes through repository structs that own a connection pool \
             reference. Each entity has its own repository (UserRepo, OrderRepo). Repositories \
             expose domain-specific methods, not raw SQL. This keeps SQL contained and testable \
             with mock repositories.",
            0.7,
            vec!["architecture", "database", "testing"],
        ),
        (
            MemoryKind::Observation,
            "[demo] Users abandon onboarding at the email verification step",
            "Analytics show 40% drop-off at email verification. Users sign up, receive the \
             confirmation email (delivery confirmed via SendGrid), but never click the link. \
             Hypothesis: the email lands in spam, or users expect magic-link login instead.",
            0.6,
            vec!["ux", "onboarding", "analytics"],
        ),
        (
            MemoryKind::Lesson,
            "[demo] Always add database indexes before load testing",
            "Spent two days debugging slow queries that turned out to need a composite index \
             on (user_id, created_at). The query planner was doing full table scans on 2M rows. \
             Lesson: check EXPLAIN output for any query that filters or sorts, especially in \
             join conditions.",
            0.85,
            vec!["database", "performance", "testing"],
        ),
        (
            MemoryKind::Preference,
            "[demo] Prefer Result<T> over panicking in library code",
            "Library crates should propagate errors with Result, never panic. Use anyhow::Result \
             in applications, thiserror for library error types. Reserve unwrap() for cases with \
             proof of correctness (e.g., static regexes, known-valid parses).",
            0.7,
            vec!["rust", "error-handling", "conventions"],
        ),
        (
            MemoryKind::Fact,
            "[demo] PostgreSQL JSONB supports GIN indexes for containment queries",
            "JSONB columns with a GIN index support fast @> (contains) queries. This means \
             you can query JSON arrays and nested objects efficiently without extracting them \
             into separate tables. GIN indexes are slower to update but fast for reads.",
            0.5,
            vec!["database", "postgresql"],
        ),
        (
            MemoryKind::Todo,
            "[demo] Migrate from bcrypt to argon2id for password hashing",
            "bcrypt truncates at 72 bytes and has weaker GPU resistance than argon2id. Plan: \
             add argon2id as the default hasher, re-hash on next login, keep bcrypt as fallback \
             for un-migrated passwords. Target: next security sprint.",
            0.6,
            vec!["security", "auth", "migration"],
        ),
        (
            MemoryKind::Decision,
            "[demo] Use SQLite for local development, PostgreSQL for production",
            "SQLite for dev gives instant setup with no Docker dependency. Feature flags gate \
             PostgreSQL-specific features (LISTEN/NOTIFY, advisory locks). CI runs tests against \
             both databases. The ORM layer abstracts differences.",
            0.7,
            vec!["database", "architecture", "dx"],
        ),
        (
            MemoryKind::Pattern,
            "[demo] Structured logging with correlation IDs across services",
            "Every incoming request gets a correlation ID (X-Request-ID header or generated UUID). \
             This ID propagates through all internal service calls and appears in every log line. \
             Makes distributed tracing possible without a full tracing backend.",
            0.7,
            vec!["observability", "architecture", "microservices"],
        ),
        (
            MemoryKind::Lesson,
            "[demo] Feature flags should default to off in production",
            "Shipped a half-built feature to production because the flag defaulted to true. \
             Now every flag is off by default, requires explicit opt-in per environment, and \
             has an owner + expiration date. Stale flags get cleaned up quarterly.",
            0.8,
            vec!["devops", "conventions", "production"],
        ),
    ];

    let mut ids = Vec::new();
    for (i, (kind, title, content, importance, tags)) in demos.iter().enumerate() {
        let mut memory = Memory::new(
            title.to_string(),
            content.to_string(),
            *kind,
            user_id.to_string(),
        );
        memory.importance = *importance;
        memory.tags = tags.iter().map(|t| t.to_string()).collect();

        let embed_text = format!("{} {}", title, content);
        let embedding = embedder.embed(&embed_text).await?;
        storage
            .save_memory(&memory, Some(&embedding))
            .await
            .with_context(|| format!("failed to save demo memory {}", i + 1))?;

        history.log(
            &MemoryEvent::new(memory.id, EventAction::Created, user_id.to_string())
                .with_title(*title),
        );

        println!("  {} {}", format!("[{}/12]", i + 1).dimmed(), title.cyan());
        ids.push(memory.id);
    }

    // 5 relations between demo memories
    let relations: Vec<(usize, usize, RelationType, f32)> = vec![
        (2, 1, RelationType::Fixes, 0.95),      // Fix fixes Error
        (1, 3, RelationType::CausedBy, 0.8), // Error caused_by Pattern (pool issue from repo pattern)
        (0, 9, RelationType::Related, 0.7),  // JWT decision related to SQLite/PG decision
        (9, 0, RelationType::Contradicts, 0.5), // SQLite decision contradicts JWT (stateless vs local)
        (11, 4, RelationType::Supersedes, 0.6), // Feature flag lesson supersedes onboarding observation
    ];

    println!("\n{}", "Creating relations...".cyan());
    for (src_idx, tgt_idx, rel_type, strength) in &relations {
        let relation = MemoryRelation {
            source_id: ids[*src_idx],
            target_id: ids[*tgt_idx],
            relation_type: *rel_type,
            strength: *strength,
        };
        storage.add_relation(&relation).await?;
        println!(
            "  {} {} → {}",
            format!("{}", rel_type).magenta(),
            demos[*src_idx].1.replace(DEMO_PREFIX, "").dimmed(),
            demos[*tgt_idx].1.replace(DEMO_PREFIX, "").dimmed(),
        );
    }

    println!(
        "\n{} Created {} demo memories and {} relations.\n\nTry:\n  {} Browse interactively\n  {} Search from CLI",
        "✓".green().bold(),
        ids.len(),
        relations.len(),
        "shabka tui".cyan(),
        "shabka search \"authentication\"".cyan(),
    );

    Ok(())
}

async fn demo_clean(storage: &Storage, history: &HistoryLogger, user_id: &str) -> Result<()> {
    let timeline = storage
        .timeline(&TimelineQuery {
            limit: 500,
            ..Default::default()
        })
        .await?;

    let demo_entries: Vec<_> = timeline
        .iter()
        .filter(|e| e.title.starts_with(DEMO_PREFIX))
        .collect();

    if demo_entries.is_empty() {
        println!("{}", "No demo memories found.".yellow());
        return Ok(());
    }

    println!(
        "{}",
        format!("Removing {} demo memories...", demo_entries.len()).cyan()
    );

    for entry in &demo_entries {
        storage.delete_memory(entry.id).await?;
        history.log(
            &MemoryEvent::new(entry.id, EventAction::Deleted, user_id.to_string())
                .with_title(&entry.title),
        );
        println!("  {} {}", "×".red(), entry.title.dimmed());
    }

    println!(
        "\n{} Removed {} demo memories.",
        "✓".green().bold(),
        demo_entries.len()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------

async fn cmd_check(storage: &Storage, repair: bool) -> Result<()> {
    println!("Database Integrity Check");
    println!("========================\n");

    let report = match storage.integrity_check() {
        Some(r) => r,
        None => {
            println!("  Integrity check is only available for SQLite storage.");
            return Ok(());
        }
    };

    let missing_note = if report.missing_embeddings > 0 {
        format!(" ({} missing)", report.missing_embeddings)
    } else {
        String::new()
    };

    println!("  Memories:    {}", report.total_memories);
    println!("  Embeddings:  {}{}", report.total_embeddings, missing_note);
    println!("  Relations:   {}", report.total_relations);
    println!("  Sessions:    {}", report.total_sessions);
    println!(
        "  SQLite:      {}",
        if report.sqlite_integrity_ok {
            "ok"
        } else {
            "FAILED"
        }
    );

    let has_issues = !report.orphaned_embeddings.is_empty()
        || !report.broken_relations.is_empty()
        || report.missing_embeddings > 0;

    if has_issues {
        println!("\n  Issues:");
        if !report.orphaned_embeddings.is_empty() {
            println!(
                "    {} orphaned embeddings",
                report.orphaned_embeddings.len()
            );
        }
        if !report.broken_relations.is_empty() {
            println!("    {} broken relations", report.broken_relations.len());
        }
        if report.missing_embeddings > 0 {
            println!(
                "    {} memories without embeddings (run `shabka reembed`)",
                report.missing_embeddings
            );
        }
    }

    if repair && (!report.orphaned_embeddings.is_empty() || !report.broken_relations.is_empty()) {
        println!("\n  Repairing...");
        if let Some((orphans, relations)) = storage.repair(&report) {
            println!("    Removed {} orphaned embeddings", orphans);
            println!("    Removed {} broken relations", relations);
        }
    }

    let pass = report.sqlite_integrity_ok
        && report.orphaned_embeddings.is_empty()
        && report.broken_relations.is_empty();

    println!("\n  Result: {}", if pass { "PASS" } else { "ISSUES FOUND" });

    Ok(())
}

// ===========================================================================
// Unit tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use shabka_core::storage::SqliteStorage;

    fn test_storage() -> Storage {
        Storage::Sqlite(SqliteStorage::open_in_memory().unwrap())
    }

    fn test_config() -> ShabkaConfig {
        ShabkaConfig::default_config()
    }

    fn test_embedder(config: &ShabkaConfig) -> EmbeddingService {
        EmbeddingService::from_config(&config.embedding).unwrap()
    }

    fn test_history() -> HistoryLogger {
        HistoryLogger::new(true)
    }

    /// Save a test memory and return its ID as a string.
    async fn seed_memory(storage: &Storage, title: &str, content: &str, kind: &str) -> String {
        let mem = Memory::new(
            title.to_string(),
            content.to_string(),
            kind.parse().unwrap_or(MemoryKind::Observation),
            "test-user".to_string(),
        );
        let id = mem.id;
        let config = test_config();
        let embedder = test_embedder(&config);
        let embedding = embedder.embed(&mem.embedding_text()).await.ok();
        storage
            .save_memory(&mem, embedding.as_deref())
            .await
            .unwrap();
        id.to_string()
    }

    // -----------------------------------------------------------------------
    // search
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_search_no_results() {
        let storage = test_storage();
        let config = test_config();
        let embedder = test_embedder(&config);
        let result = cmd_search(
            &storage,
            &embedder,
            "test-user",
            "nonexistent query",
            None,
            None,
            None,
            None,
            true,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cmd_search_with_results() {
        let storage = test_storage();
        let config = test_config();
        let embedder = test_embedder(&config);
        seed_memory(
            &storage,
            "Rust borrow checker rules",
            "The borrow checker enforces ownership and lifetime rules at compile time.",
            "lesson",
        )
        .await;

        let result = cmd_search(
            &storage,
            &embedder,
            "test-user",
            "borrow checker",
            None,
            Some(5),
            None,
            None,
            false,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cmd_search_json() {
        let storage = test_storage();
        let config = test_config();
        let embedder = test_embedder(&config);
        seed_memory(
            &storage,
            "JSON output test alpha",
            "This memory tests the JSON output mode for search results.",
            "observation",
        )
        .await;

        let result = cmd_search(
            &storage,
            &embedder,
            "test-user",
            "json output",
            None,
            Some(5),
            None,
            None,
            true,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // get
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_get_found() {
        let storage = test_storage();
        let id = seed_memory(
            &storage,
            "Get test memory bravo",
            "A memory used to test the get command with JSON output.",
            "fact",
        )
        .await;
        let result = cmd_get(&storage, &id, true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cmd_get_not_found() {
        let storage = test_storage();
        let fake_id = uuid::Uuid::now_v7().to_string();
        let result = cmd_get(&storage, &fake_id, true).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // list
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_list_empty() {
        let storage = test_storage();
        let result = cmd_list(&storage, None, None, None, 20, true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cmd_list_with_filter() {
        let storage = test_storage();
        seed_memory(
            &storage,
            "List filter decision charlie",
            "A decision memory for testing list filters.",
            "decision",
        )
        .await;
        seed_memory(
            &storage,
            "List filter error delta",
            "An error memory for testing list filters.",
            "error",
        )
        .await;

        // Filter to only decision kind
        let result = cmd_list(&storage, Some("decision".to_string()), None, None, 20, true).await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // status
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_status() {
        let storage = test_storage();
        let config = test_config();
        seed_memory(
            &storage,
            "Status test memory echo",
            "This memory exists so that status has something to count.",
            "observation",
        )
        .await;
        let result = cmd_status(&storage, &config, "test-user").await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // delete
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_delete_single() {
        let storage = test_storage();
        let history = test_history();
        let id = seed_memory(
            &storage,
            "Delete me foxtrot",
            "A memory that will be deleted in this test.",
            "observation",
        )
        .await;

        let result = cmd_delete(
            &storage,
            &history,
            "test-user",
            Some(id),
            None,
            None,
            None,
            false,
            true,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cmd_delete_bulk_no_confirm() {
        let storage = test_storage();
        let history = test_history();
        seed_memory(
            &storage,
            "Bulk delete golf",
            "This memory should not actually be deleted since confirm is false.",
            "error",
        )
        .await;

        let result = cmd_delete(
            &storage,
            &history,
            "test-user",
            None,
            Some("error".to_string()),
            None,
            None,
            false, // no --confirm
            true,
        )
        .await;
        assert!(result.is_err(), "bulk delete without --confirm should fail");
    }

    // -----------------------------------------------------------------------
    // verify
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_verify() {
        let storage = test_storage();
        let history = test_history();
        let id = seed_memory(
            &storage,
            "Verify me hotel",
            "A memory whose verification status will be changed.",
            "fact",
        )
        .await;
        let result = cmd_verify(&storage, &history, "test-user", &id, "verified").await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // history
    // -----------------------------------------------------------------------

    #[test]
    fn test_cmd_history() {
        let history = test_history();
        // cmd_history is sync; with no prior events it should print "no events"
        let result = cmd_history(&history, None, 20, true);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // prune
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_prune_dry_run() {
        let storage = test_storage();
        let history = test_history();
        seed_memory(
            &storage,
            "Prune candidate india",
            "A memory that might be pruned during a dry run test.",
            "observation",
        )
        .await;

        let result = cmd_prune(&storage, &history, "test-user", 90, true, false).await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // chain
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_chain_no_relations() {
        let storage = test_storage();
        let id = seed_memory(
            &storage,
            "Chain start juliet",
            "An isolated memory with no relations for chain traversal.",
            "pattern",
        )
        .await;

        let result = cmd_chain(&storage, &id, None, 5, true).await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // export / import roundtrip
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_export_import_roundtrip() {
        let storage = test_storage();
        let config = test_config();
        let embedder = test_embedder(&config);
        let history = test_history();

        seed_memory(
            &storage,
            "Export roundtrip kilo",
            "This memory will be exported and then imported back.",
            "lesson",
        )
        .await;

        // Export to a temp file
        let tmp_path =
            std::env::temp_dir().join(format!("shabka-test-export-{}.json", uuid::Uuid::now_v7()));
        let tmp_str = tmp_path.to_str().unwrap();

        let export_result = cmd_export(&storage, tmp_str, "private", None, false).await;
        assert!(export_result.is_ok(), "export failed: {:?}", export_result);

        // Import into a fresh storage
        let storage2 = test_storage();
        let import_result = cmd_import(&storage2, &embedder, "test-user", tmp_str, &history).await;
        assert!(import_result.is_ok(), "import failed: {:?}", import_result);

        // Verify the imported memory exists
        let entries = storage2
            .timeline(&TimelineQuery {
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            !entries.is_empty(),
            "imported storage should have at least one memory"
        );

        // Clean up temp file
        let _ = std::fs::remove_file(&tmp_path);
    }

    // -----------------------------------------------------------------------
    // assess
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_assess() {
        let storage = test_storage();
        let config = test_config();
        seed_memory(
            &storage,
            "Assess target lima",
            "A memory to be assessed for quality issues.",
            "observation",
        )
        .await;

        let result = cmd_assess(&storage, None, &config.graph, None, false, true).await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // context-pack
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_context_pack() {
        let storage = test_storage();
        let config = test_config();
        let embedder = test_embedder(&config);
        seed_memory(
            &storage,
            "Context pack mike",
            "A memory that should be included in the context pack.",
            "preference",
        )
        .await;

        let result = cmd_context_pack(
            &storage,
            &embedder,
            "test-user",
            "context",
            2000,
            None,
            None,
            None,
            true,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // demo
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cmd_demo_and_clean() {
        let storage = test_storage();
        let config = test_config();
        let embedder = test_embedder(&config);
        let history = test_history();

        // Create demo data
        let create_result = cmd_demo(&storage, &embedder, "test-user", &history, false).await;
        assert!(
            create_result.is_ok(),
            "demo create failed: {:?}",
            create_result
        );

        // Verify demo data exists
        let entries = storage
            .timeline(&TimelineQuery {
                limit: 500,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            entries.iter().any(|e| e.title.starts_with(DEMO_PREFIX)),
            "should have demo memories after demo create"
        );

        // Clean demo data
        let clean_result = cmd_demo(&storage, &embedder, "test-user", &history, true).await;
        assert!(
            clean_result.is_ok(),
            "demo clean failed: {:?}",
            clean_result
        );

        // Verify demo data is gone
        let entries_after = storage
            .timeline(&TimelineQuery {
                limit: 500,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            !entries_after
                .iter()
                .any(|e| e.title.starts_with(DEMO_PREFIX)),
            "should have no demo memories after demo --clean"
        );
    }
}
