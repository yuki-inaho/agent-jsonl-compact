use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "agent-jsonl-compact",
    version,
    about = "Codex CLI / Claude Code / OpenCode session JSONL を compact JSONL / Markdown / summary JSON へ抽出します(既定は compact JSONL のみ)。",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// サブコマンド。未指定時は抽出(従来動作)です。
    #[command(subcommand)]
    pub command: Option<Command>,

    /// rollout-*.jsonl / Claude Code <uuid>.jsonl / OpenCode run JSONL。抽出時は必須です。
    #[arg(short = 'i', long = "input")]
    pub input: Option<PathBuf>,

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

    /// 出力形式。既定は jsonl(.clean.jsonl のみ)です。
    #[arg(long = "format-out", value_enum, default_value = "jsonl")]
    pub format_out: OutputFormat,

    /// summary JSON(.summary.json)も出力します。既定では出力しません。
    #[arg(long = "summary")]
    pub summary: bool,

    /// 形式とレコード型分布だけ表示して終了します。
    #[arg(long = "stats")]
    pub stats: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// agent-jsonl-compact-reader スキルを ~/.claude/skills と ~/.codex/skills へインストールします。
    InstallSkills {
        /// Claude Code(~/.claude/skills)のみへインストールします。
        #[arg(long, conflicts_with = "codex_only")]
        claude_only: bool,

        /// Codex(~/.codex/skills)のみへインストールします。
        #[arg(long)]
        codex_only: bool,
    },
    /// 会話候補(session)を探します。探索rootは明示指定または既定root($HOME配下)です。
    Sessions(SessionsArgs),
    /// session発言をboundedに読みます。
    View(QueryArgs),
    /// 本文を検索し、ヒットと前後eventを返します。
    Search(SearchArgs),
    /// バージョンと再現可能なsource release dateを表示します。
    Version(VersionArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum QueryOutput {
    Text,
    Jsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum QueryScope {
    Messages,
    Tools,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum QueryProvider {
    Codex,
    ClaudeCode,
    #[value(name = "opencode")]
    OpenCode,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum VersionOutput {
    Text,
    Json,
}

#[derive(Debug, Clone, clap::Args)]
pub struct VersionArgs {
    /// text(既定)または json。
    #[arg(long = "output", value_enum, default_value = "text")]
    pub output: VersionOutput,
}

/// `view` と `search` の共通入力・filter・表示上限。legacy抽出の`-o`等は受け付けない。
#[derive(Debug, Clone, clap::Args)]
pub struct SharedQueryArgs {
    /// 入力JSONL。provider付き`--session`と排他です。
    #[arg(short = 'i', long = "input", conflicts_with = "session")]
    pub input: Option<std::path::PathBuf>,

    /// `provider:SESSION_ID` 形式のsession指定。`-i`と排他、`--root`探索を使って解決します。
    #[arg(long = "session")]
    pub session: Option<String>,

    /// session探索に加えるroot(繰返し可)。
    #[arg(long = "root")]
    pub root: Vec<std::path::PathBuf>,

    /// 入力形式。省略時はauto(内容から判定、不明はfail closedでrc1)。
    #[arg(long = "format", value_enum)]
    pub format: Option<SessionFormat>,

    /// 表示対象の範囲。省略時はmessages(reasoning/system/developer/compactionは常に除外)。
    #[arg(long = "scope", value_enum)]
    pub scope: Option<QueryScope>,

    /// role絞り込み(カンマ区切り: user,assistant,developer,system)。
    #[arg(long = "role")]
    pub role: Option<String>,

    /// phase絞り込み(カンマ区切り)。
    #[arg(long = "phase")]
    pub phase: Option<String>,

    /// 指定時刻以降(offset付きRFC3339、境界を含む)。
    #[arg(long = "since")]
    pub since: Option<String>,

    /// 指定時刻より前(offset付きRFC3339、境界を含まない)。
    #[arg(long = "until")]
    pub until: Option<String>,

    /// 前回返されたpage cursor。
    #[arg(long = "cursor")]
    pub cursor: Option<String>,

    /// 前回返されたappend cursor(追記のみ読む)。
    #[arg(long = "after-cursor")]
    pub after_cursor: Option<String>,

    /// 単一eventの再読。
    #[arg(long = "event")]
    pub event: Option<String>,

    /// 出力表現。text(既定)またはjsonl。
    #[arg(long = "output", value_enum, default_value = "text")]
    pub output: QueryOutput,

    /// 主表示event数(view)またはヒット数(search)。
    #[arg(long = "limit")]
    pub limit: Option<usize>,

    /// 主表示event数の末尾N件(filter後、原文順)。
    #[arg(long = "tail")]
    pub tail: Option<usize>,

    /// 主表示+context+pairを含む総event上限。
    #[arg(long = "max-events")]
    pub max_events: Option<usize>,

    /// stdout+stderrの合計byte予算。
    #[arg(long = "max-output-bytes")]
    pub max_output_bytes: Option<usize>,

    /// 1レコードの処理上限byte。
    #[arg(long = "max-record-bytes")]
    pub max_record_bytes: Option<u64>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct QueryArgs {
    #[command(flatten)]
    pub shared: SharedQueryArgs,

    /// 対応toolの呼出しと結果をcall_idで併記します。
    #[arg(long = "with-pair")]
    pub with_pair: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct SearchArgs {
    #[command(flatten)]
    pub shared: SharedQueryArgs,

    /// 検索語(繰返し指定はOR)。既定はliteral。cursor再開時は省略可(意味的filterを再利用)。
    #[arg(long = "query")]
    pub query: Vec<String>,

    /// `--query` を正規表現として解釈します。省略時はカーソルの値を再利用します。
    #[arg(long = "regex", num_args = 0..=1, default_missing_value = "true")]
    pub regex: Option<bool>,

    /// ヒット前後のcontext event数(hard scope内)。省略時はカーソルの値を再利用します。
    #[arg(long = "context")]
    pub context: Option<usize>,

    /// 対応toolの呼出しと結果をcall_idで併記します。
    #[arg(long = "with-pair")]
    pub with_pair: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub struct SessionsArgs {
    /// 探索root(繰返し可)。未指定時はprovider既定root($HOME配下)。
    #[arg(long = "root")]
    pub root: Vec<std::path::PathBuf>,

    /// 探索provider。既定はcodexとclaude_codeです。
    #[arg(long = "provider", value_enum, default_value = "all")]
    pub provider: QueryProvider,

    /// metadata上のcwd完全一致で絞り込みます。
    #[arg(long = "cwd")]
    pub cwd: Option<String>,

    /// `--cwd` の子ディレクトリも含めます。
    #[arg(long = "cwd-children")]
    pub cwd_children: bool,

    /// 返す候補数。
    #[arg(long = "limit", default_value_t = 20)]
    pub limit: usize,

    /// 走査するfile数の上限。
    #[arg(long = "scan-limit", default_value_t = 10000)]
    pub scan_limit: usize,

    /// 出力表現。text(既定)またはjsonl。
    #[arg(long = "output", value_enum, default_value = "text")]
    pub output: QueryOutput,

    /// stdout+stderrの合計byte予算。
    #[arg(long = "max-output-bytes")]
    pub max_output_bytes: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum SessionFormat {
    Auto,
    Codex,
    ClaudeCode,
    #[value(name = "opencode")]
    OpenCode,
}

impl SessionFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionFormat::Auto => "auto",
            SessionFormat::Codex => "codex",
            SessionFormat::ClaudeCode => "claude_code",
            SessionFormat::OpenCode => "opencode",
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
