//! Syntax checking for the editor: compile a script without running it.

use mlua::Lua;

/// Compile `source` as the chunk `path` (the name error messages show). `Err` holds Lua's
/// message, e.g. `[string "Assets/Scripts/A.lua"]:3: '=' expected near 'x'`.
pub fn check(source: &str, path: &str) -> Result<(), String> {
    let lua = Lua::new();
    lua.load(source)
        .set_name(path)
        .into_function()
        .map(|_| ())
        .map_err(|e| match e {
            mlua::Error::SyntaxError { message, .. } => message,
            other => other.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_line_and_message_without_running() {
        assert!(check("error('not run')\nreturn {}", "Assets/A.lua").is_ok());
        let err = check("local x = \nfunction", "Assets/Scripts/A.lua").unwrap_err();
        assert!(
            err.starts_with("[string \"Assets/Scripts/A.lua\"]:2:"),
            "{err}"
        );
    }
}
