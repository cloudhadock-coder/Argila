//! Leitura do .dproj (MSBuild/XML) sem executar nada: so o XML e lido, `<Import>` e scripts sao
//! ignorados. Edicoes de `DCCReference` sao feitas por recorte no texto original, entao
//! comentarios, condicoes e elementos que o Argila nao conhece continuam exatamente iguais.

use std::collections::BTreeMap;

use roxmltree::{Document, Node};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildConfig {
    pub name: String,
    pub key: String,
    pub parent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyGroup {
    pub condition: Option<String>,
    /// Chave da condicao (`Base`, `Base_Win32`, `Cfg_1`, `Cfg_1_Win64`...), quando reconhecida.
    pub scope: Option<String>,
    /// (nome, valor, condicao propria do elemento)
    pub properties: Vec<(String, String, Option<String>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DccReference {
    pub include: String,
    pub form: Option<String>,
    pub form_type: Option<String>,
    pub design_class: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dproj {
    pub groups: Vec<PropertyGroup>,
    pub configurations: Vec<BuildConfig>,
    /// Plataformas declaradas em ProjectExtensions (nome, ativa).
    pub platforms: Vec<(String, bool)>,
    pub references: Vec<DccReference>,
    /// Outros itens de ItemGroup (RcCompile, None, DelphiCompile...): (elemento, Include).
    pub other_items: Vec<(String, String)>,
    /// Elementos de nivel superior que o Argila nao interpreta (preservados ao editar).
    pub unknown_elements: Vec<String>,
}

fn text_of(node: Node) -> String {
    node.text().unwrap_or_default().trim().to_owned()
}

fn child_text(node: Node, name: &str) -> Option<String> {
    node.children()
        .find(|c| c.tag_name().name() == name)
        .map(text_of)
        .filter(|s| !s.is_empty())
}

/// `'$(Config)'=='Release' or '$(Cfg_1)'!=''` -> `Cfg_1` (a ultima variavel testada contra vazio).
pub fn condition_scope(condition: &str) -> Option<String> {
    let mut scope = None;
    let mut rest = condition;
    while let Some(p) = rest.find("'$(") {
        let after = &rest[p + 3..];
        let Some(end) = after.find(")'") else { break };
        if after[end + 2..].trim_start().starts_with("!=''") {
            scope = Some(after[..end].to_owned());
        }
        rest = &after[end + 2..];
    }
    scope
}

pub fn parse(text: &str) -> Result<Dproj, String> {
    let doc = Document::parse(text).map_err(|e| format!("invalid XML: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "Project" {
        return Err(format!(
            "root element is <{}>, expected <Project>",
            root.tag_name().name()
        ));
    }
    let mut d = Dproj::default();
    for node in root.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "PropertyGroup" => {
                let condition = node.attribute("Condition").map(str::to_owned);
                let scope = condition.as_deref().and_then(condition_scope);
                let properties = node
                    .children()
                    .filter(Node::is_element)
                    .map(|p| {
                        (
                            p.tag_name().name().to_owned(),
                            text_of(p),
                            p.attribute("Condition").map(str::to_owned),
                        )
                    })
                    .collect();
                d.groups.push(PropertyGroup {
                    condition,
                    scope,
                    properties,
                });
            }
            "ItemGroup" => {
                for item in node.children().filter(Node::is_element) {
                    let include = item.attribute("Include").unwrap_or_default().to_owned();
                    match item.tag_name().name() {
                        "DCCReference" => d.references.push(DccReference {
                            include,
                            form: child_text(item, "Form"),
                            form_type: child_text(item, "FormType"),
                            design_class: child_text(item, "DesignClass"),
                        }),
                        "BuildConfiguration" => d.configurations.push(BuildConfig {
                            name: include,
                            key: child_text(item, "Key").unwrap_or_default(),
                            parent: child_text(item, "CfgParent"),
                        }),
                        other => d.other_items.push((other.to_owned(), include)),
                    }
                }
            }
            "ProjectExtensions" => {
                for p in node
                    .descendants()
                    .filter(|n| n.tag_name().name() == "Platform")
                {
                    if let Some(name) = p.attribute("value") {
                        d.platforms
                            .push((name.to_owned(), text_of(p).eq_ignore_ascii_case("true")));
                    }
                }
            }
            "Import" | "Target" => {}
            other => d.unknown_elements.push(other.to_owned()),
        }
    }
    Ok(d)
}

impl Dproj {
    /// Propriedade do grupo sem condicao (ProjectGuid, MainSource, ProjectVersion...).
    pub fn global(&self, name: &str) -> Option<&str> {
        self.groups
            .iter()
            .filter(|g| g.condition.is_none())
            .flat_map(|g| &g.properties)
            .find(|(n, _, _)| n == name)
            .map(|(_, v, _)| v.as_str())
            .filter(|v| !v.is_empty())
    }

    /// Nome legivel de um escopo: `Cfg_1_Win32` -> `Release · Win32`.
    pub fn scope_label(&self, scope: &str) -> String {
        if scope == "Base" {
            return "Base (all configurations)".to_owned();
        }
        for (name, _) in &self.platforms {
            if let Some(cfg) = scope.strip_suffix(&format!("_{name}")) {
                return format!(
                    "{} · {name}",
                    self.scope_label(cfg).replace(" (all configurations)", "")
                );
            }
        }
        self.configurations
            .iter()
            .find(|c| c.key == scope)
            .map_or_else(|| scope.to_owned(), |c| c.name.clone())
    }

    pub fn config_names(&self) -> Vec<String> {
        self.configurations
            .iter()
            .filter(|c| c.key != "Base")
            .map(|c| c.name.clone())
            .collect()
    }

    pub fn platform_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .platforms
            .iter()
            .filter(|p| p.1)
            .map(|p| p.0.clone())
            .collect();
        if names.is_empty() {
            names = self
                .global("Platform")
                .map(str::to_owned)
                .into_iter()
                .collect();
        }
        names
    }

    /// Valores efetivos para uma configuracao e plataforma, seguindo a heranca do Delphi
    /// (projeto -> Base -> Base_Plataforma -> Cfg... -> Cfg_Plataforma) e expandindo `$(Nome)`
    /// com os valores herdados. Variaveis desconhecidas (`$(BDS)`...) ficam como estao.
    pub fn effective(&self, config: &str, platform: &str) -> BTreeMap<String, String> {
        let mut chain = vec![String::new(), "Base".to_owned(), format!("Base_{platform}")];
        let mut cfgs = Vec::new();
        let mut key = self
            .configurations
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(config))
            .map(|c| c.key.clone());
        while let Some(k) = key.filter(|k| k != "Base" && !cfgs.contains(k)) {
            key = self
                .configurations
                .iter()
                .find(|c| c.key == k)
                .and_then(|c| c.parent.clone());
            cfgs.push(k);
        }
        for k in cfgs.iter().rev() {
            chain.push(k.clone());
            chain.push(format!("{k}_{platform}"));
        }
        let mut vals = BTreeMap::new();
        vals.insert("Config".to_owned(), config.to_owned());
        vals.insert("Platform".to_owned(), platform.to_owned());
        for scope in &chain {
            for g in self
                .groups
                .iter()
                .filter(|g| g.scope.as_deref().unwrap_or_default() == scope)
            {
                if g.condition.is_some() && g.scope.is_none() {
                    continue;
                }
                for (name, value, own_condition) in &g.properties {
                    if own_condition.is_some() && vals.contains_key(name) {
                        continue; // `<Config Condition="'$(Config)'==''">` so vale como padrao
                    }
                    let expanded = expand(value, &vals);
                    vals.insert(name.clone(), expanded);
                }
            }
        }
        let markers: Vec<&String> = chain.iter().collect();
        vals.retain(|k, _| !markers.contains(&k) && k != "CfgParent" && k != "Base");
        vals
    }
}

fn expand(value: &str, vals: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(p) = rest.find("$(") {
        out.push_str(&rest[..p]);
        let after = &rest[p + 2..];
        match after.find(')') {
            Some(end) => {
                let name = &after[..end];
                match vals.get(name) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[p..p + 2 + end + 1]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[p..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn find_reference<'a>(doc: &'a Document, include: &str) -> Option<Node<'a, 'a>> {
    doc.descendants().find(|n| {
        n.tag_name().name() == "DCCReference"
            && n.attribute("Include")
                .is_some_and(|i| i.eq_ignore_ascii_case(include))
    })
}

/// Insere uma `DCCReference` depois da ultima existente, com a mesma indentacao.
pub fn add_reference(
    text: &str,
    include: &str,
    form: Option<&str>,
    form_type: Option<&str>,
) -> Result<String, String> {
    let doc = Document::parse(text).map_err(|e| format!("invalid XML: {e}"))?;
    if find_reference(&doc, include).is_some() {
        return Err(format!("{include} is already referenced in the .dproj"));
    }
    let last = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "DCCReference")
        .last()
        .ok_or("the .dproj has no DCCReference items to follow")?;
    let start = last.range().start;
    let line_start = text[..start].rfind('\n').map_or(0, |p| p + 1);
    let indent = &text[line_start..start];
    let inc = xml_escape(include);
    let element = match form {
        Some(f) => format!(
            "<DCCReference Include=\"{inc}\">\n{indent}    <Form>{}</Form>\n{indent}    <FormType>{}</FormType>\n{indent}</DCCReference>",
            xml_escape(f),
            xml_escape(form_type.unwrap_or("dfm"))
        ),
        None => format!("<DCCReference Include=\"{inc}\"/>"),
    };
    let mut out = text.to_owned();
    out.insert_str(last.range().end, &format!("\n{indent}{element}"));
    Ok(out)
}

/// Remove a `DCCReference` (e a quebra de linha antes dela). O arquivo da unit nao e tocado.
pub fn remove_reference(text: &str, include: &str) -> Result<String, String> {
    let doc = Document::parse(text).map_err(|e| format!("invalid XML: {e}"))?;
    let node = find_reference(&doc, include)
        .ok_or_else(|| format!("{include} is not referenced in the .dproj"))?;
    let r = node.range();
    let start = text[..r.start]
        .rfind('\n')
        .filter(|p| text[p + 1..r.start].trim().is_empty())
        .unwrap_or(r.start);
    let mut out = text.to_owned();
    out.replace_range(start..r.end, "");
    Ok(out)
}

/// Troca o `Include` de uma `DCCReference`.
pub fn rename_reference(text: &str, old: &str, new: &str) -> Result<String, String> {
    let doc = Document::parse(text).map_err(|e| format!("invalid XML: {e}"))?;
    let node = find_reference(&doc, old)
        .ok_or_else(|| format!("{old} is not referenced in the .dproj"))?;
    let r = node.range();
    let tag = &text[r.clone()];
    let attr = tag.find("Include=").ok_or("Include attribute not found")?;
    let quote = tag[attr + 8..]
        .chars()
        .next()
        .ok_or("malformed attribute")?;
    let value_start = r.start + attr + 9;
    let value_end = value_start
        + text[value_start..]
            .find(quote)
            .ok_or("malformed attribute")?;
    let mut out = text.to_owned();
    out.replace_range(value_start..value_end, &xml_escape(new));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const DPROJ: &str = r#"<Project xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
    <!-- comentario do usuario -->
    <PropertyGroup>
        <ProjectGuid>{7DA46F73-0AC6-4D77-AF6F-BFEBE7BB42BC}</ProjectGuid>
        <MainSource>Teste.dpr</MainSource>
        <Config Condition="'$(Config)'==''">Debug</Config>
        <ProjectVersion>20.4</ProjectVersion>
        <Platform Condition="'$(Platform)'==''">Win32</Platform>
    </PropertyGroup>
    <PropertyGroup Condition="'$(Config)'=='Base' or '$(Base)'!=''">
        <Base>true</Base>
        <DCC_UnitSearchPath>..\Comum;$(DCC_UnitSearchPath)</DCC_UnitSearchPath>
        <DCC_Define>APP;$(DCC_Define)</DCC_Define>
        <DCC_ExeOutput>.\$(Platform)\$(Config)</DCC_ExeOutput>
    </PropertyGroup>
    <PropertyGroup Condition="('$(Platform)'=='Win32' and '$(Base)'=='true') or '$(Base_Win32)'!=''">
        <Base_Win32>true</Base_Win32>
        <DCC_Namespace>Winapi;$(DCC_Namespace)</DCC_Namespace>
    </PropertyGroup>
    <PropertyGroup Condition="'$(Config)'=='Debug' or '$(Cfg_1)'!=''">
        <Cfg_1>true</Cfg_1>
        <DCC_Define>DEBUG;$(DCC_Define)</DCC_Define>
    </PropertyGroup>
    <PropertyGroup Condition="('$(Platform)'=='Win64' and '$(Cfg_1)'=='true') or '$(Cfg_1_Win64)'!=''">
        <DCC_Define>WIN64DBG;$(DCC_Define)</DCC_Define>
    </PropertyGroup>
    <ItemGroup>
        <DelphiCompile Include="$(MainSource)">
            <MainSource>MainSource</MainSource>
        </DelphiCompile>
        <DCCReference Include="Unit_Dm.pas">
            <Form>Dm</Form>
            <FormType>dfm</FormType>
            <DesignClass>TDataModule</DesignClass>
        </DCCReference>
        <DCCReference Include="..\Comum\Unit_Ação.pas"/>
        <BuildConfiguration Include="Base">
            <Key>Base</Key>
        </BuildConfiguration>
        <BuildConfiguration Include="Debug">
            <Key>Cfg_1</Key>
            <CfgParent>Base</CfgParent>
        </BuildConfiguration>
    </ItemGroup>
    <ProjectExtensions>
        <BorlandProject>
            <Platforms>
                <Platform value="Win32">True</Platform>
                <Platform value="Win64">False</Platform>
            </Platforms>
        </BorlandProject>
    </ProjectExtensions>
    <Import Project="$(BDS)\Bin\CodeGear.Delphi.Targets" Condition="Exists('$(BDS)\Bin\CodeGear.Delphi.Targets')"/>
    <SomethingNew Version="99"/>
</Project>
"#;

    #[test]
    fn parses_configurations_platforms_and_references() {
        let d = parse(DPROJ).unwrap();
        assert_eq!(d.global("MainSource"), Some("Teste.dpr"));
        assert_eq!(d.config_names(), ["Debug"]);
        assert_eq!(d.platform_names(), ["Win32"]);
        assert_eq!(d.references.len(), 2);
        assert_eq!(d.references[0].form.as_deref(), Some("Dm"));
        assert_eq!(d.references[1].include, "..\\Comum\\Unit_Ação.pas");
        assert_eq!(d.unknown_elements, ["SomethingNew"]);
        assert_eq!(d.scope_label("Cfg_1_Win64"), "Debug · Win64");
        assert_eq!(
            condition_scope(
                "('$(Platform)'=='Win32' and '$(Base)'=='true') or '$(Base_Win32)'!=''"
            )
            .as_deref(),
            Some("Base_Win32")
        );
    }

    #[test]
    fn effective_values_follow_inheritance() {
        let d = parse(DPROJ).unwrap();
        let e = d.effective("Debug", "Win32");
        assert_eq!(e["DCC_Define"], "DEBUG;APP;$(DCC_Define)");
        assert_eq!(e["DCC_ExeOutput"], ".\\Win32\\Debug");
        assert_eq!(e["DCC_UnitSearchPath"], "..\\Comum;$(DCC_UnitSearchPath)");
        assert_eq!(e["DCC_Namespace"], "Winapi;$(DCC_Namespace)");
        assert!(!e.contains_key("Cfg_1") && !e.contains_key("Base"));
        let e64 = d.effective("Debug", "Win64");
        assert_eq!(e64["DCC_Define"], "WIN64DBG;DEBUG;APP;$(DCC_Define)");
        assert!(!e64.contains_key("DCC_Namespace"));
    }

    #[test]
    fn reference_edits_keep_comments_and_unknown_elements() {
        let added = add_reference(DPROJ, "Unit_Nova.pas", Some("Form_Nova"), None).unwrap();
        let expected_insert = "\n        <DCCReference Include=\"Unit_Nova.pas\">\n            <Form>Form_Nova</Form>\n            <FormType>dfm</FormType>\n        </DCCReference>";
        assert_eq!(added.replacen(expected_insert, "", 1), DPROJ);
        assert_eq!(parse(&added).unwrap().references.len(), 3);
        assert!(add_reference(DPROJ, "unit_dm.pas", None, None).is_err());

        let removed = remove_reference(DPROJ, "..\\Comum\\Unit_Ação.pas").unwrap();
        assert_eq!(
            removed,
            DPROJ.replace(
                "\n        <DCCReference Include=\"..\\Comum\\Unit_Ação.pas\"/>",
                ""
            )
        );
        assert!(
            removed.contains("<!-- comentario do usuario -->") && removed.contains("<SomethingNew")
        );

        let renamed = rename_reference(DPROJ, "Unit_Dm.pas", "Unit_Dados.pas").unwrap();
        assert_eq!(
            renamed,
            DPROJ.replace("Include=\"Unit_Dm.pas\"", "Include=\"Unit_Dados.pas\"")
        );
    }
}
