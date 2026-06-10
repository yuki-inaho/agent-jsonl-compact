use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "agent-jsonl-compact",
    about = "Codex CLI / Claude Code session JSONL を compact JSONL・Markdown・summary JSON へ抽出します。"
)]
pub struct Cli {
    /// rollout-*.jsonl / Claude Code <uuid>.jsonl
    #[arg(short = 'i', long = "input")]
    pub input: PathBuf,

    /// 出力先。未指定時はカレントディレクトリです。
    #[arg(short = 'o', long = "out-dir")]
    pub out_dir: Option<PathBuf>,

    /// 出力ベース名。未指定時は入力ファイル名の stem です。
    #[arg(long = "name")]
    pub name: Option<String>,

    /// 入力形式。auto は先頭行から自動判定します。
    #[arg(long = "format", value_enum, default_value = "auto")]
    pub format: SessionFormat,

    /// 本文 truncate 長。0 は全文保持です。
    #[arg(long = "msg-chars", default_value_t = 0)]
    pub msg_chars: usize,

    /// ツール出力 truncate 長。0 は全文保持です。
    #[arg(long = "out-chars", default_value_t = 0)]
    pub out_chars: usize,

    /// Codex のみ有効。terminal は TUI 相当、api は response_item 本文中心、both は両方です。
    #[arg(long = "channel", value_enum, default_value = "terminal")]
    pub channel: Channel,

    /// Codex token_count を残します。既定では捨てます。
    #[arg(long = "keep-token-count")]
    pub keep_token_count: bool,

    /// 連続重複本文と重複 web_search の畳み込みを無効化します。
    #[arg(long = "no-dedup")]
    pub no_dedup: bool,

    /// 肥大ツール出力を件数と先頭行だけへ畳みます。
    #[arg(long = "elide-outputs")]
    pub elide_outputs: bool,

    /// 出力形式。summary JSON は常に出力します。
    #[arg(long = "format-out", value_enum, default_value = "both")]
    pub format_out: OutputFormat,

    /// 形式とレコード型分布だけ表示して終了します。
    #[arg(long = "stats")]
    pub stats: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum SessionFormat {
    Auto,
    Codex,
    ClaudeCode,
}

impl SessionFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionFormat::Auto => "auto",
            SessionFormat::Codex => "codex",
            SessionFormat::ClaudeCode => "claude_code",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum Channel {
    Terminal,
    Api,
    Both,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Terminal => "terminal",
            Channel::Api => "api",
            Channel::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum OutputFormat {
    Jsonl,
    Md,
    Both,
}
