//! Language identification without reading or recoding a document.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolingPlan {
    ProgressBridge,
    OptionalLsp,
    LocalOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub filenames: &'static [&'static str],
    pub line_comment: Option<&'static str>,
    pub tooling_plan: ToolingPlan,
}

pub static PROFILES: &[LanguageProfile] = &[
    LanguageProfile { id:"abl", name:"Progress ABL", extensions:&["p","w","i","cls"], filenames:&[], line_comment:None, tooling_plan:ToolingPlan::ProgressBridge },
    LanguageProfile { id:"csharp", name:"C#", extensions:&["cs","csx"], filenames:&[], line_comment:Some("//"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"rust", name:"Rust", extensions:&["rs"], filenames:&[], line_comment:Some("//"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"javascript", name:"JavaScript / JSX", extensions:&["js","jsx","mjs","cjs"], filenames:&[], line_comment:Some("//"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"typescript", name:"TypeScript / TSX", extensions:&["ts","tsx","mts","cts"], filenames:&[], line_comment:Some("//"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"python", name:"Python", extensions:&["py","pyw","pyi"], filenames:&[], line_comment:Some("#"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"html", name:"HTML", extensions:&["html","htm"], filenames:&[], line_comment:None, tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"css", name:"CSS", extensions:&["css"], filenames:&[], line_comment:None, tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"sql", name:"SQL", extensions:&["sql"], filenames:&[], line_comment:Some("--"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"xml", name:"XML", extensions:&["xml","xsd","xsl","xslt","csproj","props","targets"], filenames:&[], line_comment:None, tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"json", name:"JSON", extensions:&["json"], filenames:&[], line_comment:None, tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"jsonc", name:"JSON with comments", extensions:&["jsonc"], filenames:&["tsconfig.json","jsconfig.json"], line_comment:Some("//"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"markdown", name:"Markdown", extensions:&["md","markdown"], filenames:&[], line_comment:None, tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"toml", name:"TOML", extensions:&["toml"], filenames:&[], line_comment:Some("#"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"yaml", name:"YAML", extensions:&["yaml","yml"], filenames:&[], line_comment:Some("#"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"powershell", name:"PowerShell", extensions:&["ps1","psm1","psd1"], filenames:&[], line_comment:Some("#"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"shell", name:"Shell", extensions:&["sh","bash","zsh"], filenames:&[".bashrc",".zshrc"], line_comment:Some("#"), tooling_plan:ToolingPlan::OptionalLsp },
    LanguageProfile { id:"text", name:"Plain text", extensions:&["txt","log"], filenames:&[], line_comment:None, tooling_plan:ToolingPlan::LocalOnly },
];

pub fn by_id(id: &str) -> Option<&'static LanguageProfile> {
    PROFILES.iter().find(|profile| profile.id.eq_ignore_ascii_case(id))
}

pub fn detect(path: &str) -> &'static LanguageProfile {
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if let Some(profile) = PROFILES.iter().find(|profile| {
        profile.filenames.iter().any(|name| filename.eq_ignore_ascii_case(name))
    }) {
        return profile;
    }
    if let Some((stem, extension)) = filename.rsplit_once('.') {
        if !stem.is_empty() {
            if let Some(profile) = PROFILES.iter().find(|profile| {
                profile.extensions.iter().any(|ext| extension.eq_ignore_ascii_case(ext))
            }) {
                return profile;
            }
        }
    }
    by_id("text").expect("the built-in plain-text profile exists")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_extension_resolves_to_its_profile() {
        let mut extensions = HashSet::new();
        let mut ids = HashSet::new();
        for profile in PROFILES {
            assert!(ids.insert(profile.id), "duplicate language id");
            for extension in profile.extensions {
                assert!(extensions.insert(extension), "ambiguous extension");
                assert_eq!(detect(&format!("sample.{extension}")).id, profile.id);
            }
        }
    }

    #[test]
    fn explicit_filenames_precede_extension_hints() {
        assert_eq!(detect(r"C:\project\tsconfig.json").id, "jsonc");
        assert_eq!(detect("/project/package.json").id, "json");
        assert_eq!(detect(".bashrc").id, "shell");
    }

    #[test]
    fn unknowns_are_text_and_windows_paths_are_portable() {
        assert_eq!(detect(r"C:\sources\clientes.P").id, "abl");
        assert_eq!(detect("/tmp/folder.rs/file").id, "text");
        assert_eq!(detect(".rs").id, "text");
        assert_eq!(detect("unknown.blob").id, "text");
        assert_eq!(detect("Proyecto/niño.cs").id, "csharp");
    }
}
