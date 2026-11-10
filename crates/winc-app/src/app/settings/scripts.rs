//! 设置里「运行」一页：各脚本的解释器命令。
//!
//! 可运行脚本的语言固定为 [`winc_config::script_language_ids`] 列出的几种，每一行是一个语言。
//! 项目设置（`.winc/config.json`）里写的命令优先且只读，用户设置里写的命令可编辑；都没写时用
//! PATH 上的默认解释器。

use winc_i18n::t;

use super::*;

impl App {
    /// 重新计算运行页每一行上显示的内容；打开设置、修改命令之后调用。
    pub(super) fn refresh_script_rows(&mut self) {
        let project = self.project_format().unwrap_or_default();
        let ids = self.settings.languages.script.clone();
        self.settings.script_rows = ids
            .into_iter()
            .filter_map(|id| {
                let language = winc_syntax::find(id)?;
                let user = self.config.script_commands.get(id).cloned();
                let project_command = project.script_commands.get(id).cloned();
                let row = self.script_row(language, user, project_command);
                Some((Setting::ScriptCommand(id), row))
            })
            .collect();
    }

    /// 计算 `language` 的脚本解释器行上显示的内容。
    ///
    /// 项目设置中的命令优先（只读，标记来源）；否则用户设置；都没写时显示默认解释器。
    fn script_row(
        &self,
        language: &'static Language,
        user: Option<String>,
        project: Option<String>,
    ) -> FormatRow {
        let default = winc_config::script_language_command(language.id).unwrap_or("").to_owned();
        let configured = project.clone().or_else(|| user.clone());
        let tool = configured.clone().unwrap_or_else(|| default.clone());
        let detail = match (&project, &user) {
            (Some(command), _) => format!("{command} · {}", t!("format-tool-from-project")),
            (None, Some(command)) => command.clone(),
            (None, None) => t!("settings-run-default-path", command = default),
        };
        FormatRow {
            tool,
            unset: false,
            detail,
            auto: None,
            project,
            warning: None,
            install: Vec::new(),
            removable: false,
        }
    }

    /// 提交语言 `id` 的脚本解释器命令并写回用户配置；`command` 为 `None`（清空）时恢复默认。
    pub(in crate::app) fn settings_commit_script(&mut self, id: &'static str, command: Option<String>) {
        match command {
            Some(command) => {
                self.config.script_commands.insert(id.to_owned(), command);
            }
            None => {
                self.config.script_commands.remove(id);
            }
        }
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
        self.save_config();
        self.refresh_script_rows();
        self.request_redraw();
    }
}
