use zed_extension_api as zed;

struct KlogExtension;

impl zed::Extension for KlogExtension {
    fn new() -> Self {
        KlogExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let path = worktree
            .which("klog-lsp")
            .ok_or_else(|| "klog-lsp binary not found. Please run `cargo install --path .` in the extension directory.".to_string())?;

        Ok(zed::Command {
            command: path,
            args: vec![],
            env: Default::default(),
        })
    }
}

zed::register_extension!(KlogExtension);
