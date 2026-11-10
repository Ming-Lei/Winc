//! 项目的运行配置：`root` 下的 `.winc/run.json`，由用户编辑，可以随仓库共享。
//!
//! 文件内容来自项目仓库，不可信：这里只负责解析和校验，运行前是否询问用户由调用方决定（见
//! [`RunConfig::fingerprint`]）。

use std::path::{Path, PathBuf};

use winc_i18n::t;

use crate::json::Json;

mod detect;

use detect::detect_commands;

/// 一个运行配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    /// 名字，在同一个文件中唯一。
    pub name: String,
    /// 交给 shell 执行的命令行。
    pub command: String,
    /// 追加在命令行之后的参数，每项按原样传给程序。
    pub args: Vec<String>,
    /// 工作目录，相对路径相对于打开的目录；`None` 表示打开的目录。
    pub cwd: Option<String>,
    /// 额外的环境变量，每项是（键，值），按键排序。
    pub env: Vec<(String, String)>,
}

impl RunConfig {
    /// 返回内容的指纹（FNV-1a）：用户同意运行后记录，命令、目录或环境变量改变时重新询问。
    pub fn fingerprint(&self) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = OFFSET;
        let mut add = |text: &str| {
            // 字段之间加入长度，内容在相邻字段之间移动时指纹不同。
            let length = text.len().to_le_bytes();
            for byte in length.iter().chain(text.as_bytes()) {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(PRIME);
            }
        };
        add(&self.command);
        add(self.cwd.as_deref().unwrap_or_default());
        for arg in &self.args {
            add(arg);
        }
        for (key, value) in &self.env {
            add(key);
            add(value);
        }
        hash
    }
}

/// 返回运行配置文件的路径。
pub fn run_configs_path(root: &Path) -> PathBuf {
    root.join(".winc").join("run.json")
}

/// 读取 `root` 的运行配置，返回可用的配置和遇到的问题。
///
/// 文件不存在时两者都为空；单项无效时跳过该项，其余照常使用。
pub fn load_run_configs(root: &Path) -> (Vec<RunConfig>, Vec<String>) {
    match std::fs::read_to_string(run_configs_path(root)) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
        Err(e) => {
            let problem = t!("config-run-read-failed", error = e.to_string());
            (Vec::new(), vec![problem])
        }
    }
}

/// `path` 变化时是否要重新识别项目的命令：它是项目根目录 `root` 下的标记文件。
pub fn affects_detected_configs(root: &Path, path: &Path) -> bool {
    detect::is_marker(root, path)
}

/// 返回从项目文件识别出的配置：没有配置文件时直接提供，不往项目里写文件。
pub fn detected_run_configs(root: &Path) -> Vec<RunConfig> {
    let commands = detect_commands(root).into_iter();
    commands
        .map(|(name, command)| RunConfig {
            name,
            command,
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
        })
        .collect()
}

/// 配置文件不存在时新建，内容按项目类型生成；已存在时不改动。返回是否新建了文件。
///
/// # Errors
///
/// 无法创建目录或写入文件时返回对应的 `std::io::Error`。
pub fn create_run_configs(root: &Path) -> std::io::Result<bool> {
    let path = run_configs_path(root);
    if path.exists() {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::files::write_atomic(&path, &template(root))?;
    Ok(true)
}

/// 返回新建配置文件的内容，每项都写明全部字段，便于修改。
///
/// 认出项目类型时列出常用命令，否则是一个各 shell 都能运行的示例，不冒充项目的命令。
fn template(root: &Path) -> String {
    let string = |text: &str| Json::String(text.to_owned()).compact();
    let note = t!("config-run-template-note");
    let mut commands = detect_commands(root);
    if commands.is_empty() {
        commands.push(("example".to_owned(), "echo \"Hello from Winc\"".to_owned()));
    }
    let items = commands.iter().map(|(name, command)| {
        let (name, command) = (string(name), string(command));
        [
            format!("    {{\n      \"name\": {name},\n"),
            format!("      \"command\": {command},\n"),
            "      \"args\": [],\n      \"cwd\": \".\",\n".to_owned(),
            "      \"env\": {}\n    }".to_owned(),
        ]
        .concat()
    });
    let items = items.collect::<Vec<_>>().join(",\n");
    format!("{{\n  // {note}\n  \"configurations\": [\n{items}\n  ]\n}}\n")
}

fn parse(text: &str) -> (Vec<RunConfig>, Vec<String>) {
    let root = Json::parse_jsonc(text);
    let Some(object) = root.as_ref().and_then(|(root, _)| root.as_object()) else {
        return (Vec::new(), vec![t!("config-run-invalid-json").to_owned()]);
    };
    let mut problems = Vec::new();
    let mut configs: Vec<RunConfig> = Vec::new();
    for (key, value) in object {
        if key != "configurations" {
            problems.push(t!("config-run-unsupported-key", key = key.as_str()));
            continue;
        }
        let Some(items) = value.as_array() else {
            problems.push(t!("config-run-invalid-json").to_owned());
            continue;
        };
        for (index, item) in items.iter().enumerate() {
            match config(item, &mut problems) {
                None => problems.push(t!("config-run-invalid-entry", index = index + 1)),
                Some(config) if configs.iter().any(|c| c.name == config.name) => {
                    problems.push(t!("config-run-duplicate-name", name = config.name.as_str()));
                }
                Some(config) => configs.push(config),
            }
        }
    }
    (configs, problems)
}

/// 解析一项配置；缺少名字或命令、字段类型不对时返回 `None`。
fn config(item: &Json, problems: &mut Vec<String>) -> Option<RunConfig> {
    let object = item.as_object()?;
    let mut config = RunConfig {
        name: text(object.get("name")?).filter(|s| !s.trim().is_empty())?,
        command: text(object.get("command")?).filter(|s| !s.trim().is_empty())?,
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
    };
    for (key, value) in object {
        match key.as_str() {
            "name" | "command" => {}
            "args" => {
                let args = value.as_array()?.iter().map(text);
                config.args = args.collect::<Option<_>>()?;
            }
            "cwd" => config.cwd = Some(text(value)?),
            "env" => {
                for (name, value) in value.as_object()? {
                    if name.is_empty() || name.contains(['=', '\0']) {
                        return None;
                    }
                    config.env.push((name.clone(), text(value)?));
                }
            }
            _ => problems.push(t!("config-run-unsupported-key", key = key.as_str())),
        }
    }
    Some(config)
}

/// 返回字符串的内容；不是字符串或含 NUL（无法传给操作系统）时返回 `None`。
fn text(value: &Json) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.contains('\0'))
        .map(str::to_owned)
}

/// 一种可运行脚本的识别结果：解释器命令和界面显示名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Script {
    /// 交给 shell 执行的解释器命令行（可带参数，如 `node --experimental-strip-types`）。
    pub command: &'static str,
    /// 界面显示的友好名字（如「Python」「Node」），用于「运行 {工具}」菜单项。
    pub tool: &'static str,
}

/// 按文件扩展名识别脚本的解释器；不认识的扩展名返回 `None`。
///
/// 只列确定能用解释器直接运行的脚本语言；需要编译或项目上下文的（如 Rust、Java）不在此列，
/// 由 [`detected_run_configs`] 按项目文件识别。不检查解释器是否已安装：主程序的 `PATH` 不一定和
/// 用户的 shell 相同，提前判断容易误判，运行失败会显示在内置终端面板中。
pub fn script(file: &Path) -> Option<Script> {
    let extension = file.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "py" => Script {
            command: if cfg!(windows) { "python" } else { "python3" },
            tool: "Python",
        },
        "js" | "mjs" | "cjs" => Script { command: "node", tool: "Node" },
        // Node 22.6+ 直接运行 TypeScript；更老的 Node 需要 `tsx`。
        "ts" | "mts" | "cts" => Script {
            command: "node --experimental-strip-types",
            tool: "Node",
        },
        "rb" => Script { command: "ruby", tool: "Ruby" },
        "sh" | "bash" => Script { command: "bash", tool: "Bash" },
        "pl" | "pm" => Script { command: "perl", tool: "Perl" },
        "lua" => Script { command: "lua", tool: "Lua" },
        "php" => Script { command: "php", tool: "PHP" },
        "go" => Script { command: "go run", tool: "Go" },
        _ => return None,
    })
}

/// 返回运行脚本文件 `file` 的配置：解释器按扩展名识别，工作目录是文件所在目录。
///
/// 工作目录用绝对路径（相对导入、`__file__`、`process.cwd()` 等按此解析），不受是否打开目录
/// 影响。不认识的扩展名返回 `None`。
pub fn script_config(file: &Path) -> Option<RunConfig> {
    let script = script(file)?;
    let name = file.file_name()?.to_string_lossy().into_owned();
    let cwd = file.parent().map(|dir| dir.to_string_lossy().into_owned());
    Some(RunConfig {
        name: format!("Run {name}"),
        command: script.command.to_owned(),
        args: vec![file.to_string_lossy().into_owned()],
        cwd,
        env: Vec::new(),
    })
}

/// 把用户输入的参数字符串按 shell 规则拆成多项：空白分隔，支持单引号、双引号和反斜杠转义。
///
/// 每项随后按原样传给程序（[`RunConfig::args`]），引号在此已处理完毕，不再交给 shell 解析；
/// 未闭合的引号按已读到的内容处理。空输入返回空列表。
pub fn split_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    current.push(c);
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        args.push(std::mem::take(&mut current));
                    }
                }
                c => current.push(c),
            },
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests;
