use typst_as_lib::{typst_kit_options::TypstKitFontOptions, TypstEngine, TypstTemplateMainFile, TypstAsLibError};
use typst_as_lib::file_resolver::FileResolver;
use typst_syntax::{Source, FileId as TypstFileId, VirtualPath, package::PackageSpec};
use typst::foundations::Bytes;
use typst::diag::FileResult;
use std::borrow::Cow;
use std::collections::HashMap;
use base64::{engine::general_purpose::STANDARD, Engine as _};

pub struct TypstCompiler;

struct PreparedFiles {
    sources: Vec<(String, String)>,                   // project .typ: (path, content)
    binaries: Vec<(String, Vec<u8>)>,                 // project binary: (path, bytes)
    pkg_sources: Vec<(PackageSpec, String, String)>,   // pkg .typ: (spec, path, content)
    pkg_binaries: Vec<(PackageSpec, String, Vec<u8>)>, // pkg binary: (spec, path, bytes)
}

/// Custom file resolver that handles ALL package files (both sources and binaries).
/// This avoids the split between StaticSourceFileResolver and StaticFileResolver.
struct PackageFileResolver {
    sources: HashMap<TypstFileId, Source>,
    binaries: HashMap<TypstFileId, Bytes>,
}

impl PackageFileResolver {
    fn new(
        pkg_sources: &[(PackageSpec, String, String)],
        pkg_binaries: &[(PackageSpec, String, Vec<u8>)],
    ) -> Self {
        let mut sources = HashMap::new();
        let mut binaries = HashMap::new();

        for (spec, path, content) in pkg_sources {
            let fid = TypstFileId::new(Some(spec.clone()), VirtualPath::new(path));
            let source = Source::new(fid, content.clone());
            sources.insert(fid, source);
        }

        for (spec, path, data) in pkg_binaries {
            let fid = TypstFileId::new(Some(spec.clone()), VirtualPath::new(path));
            binaries.insert(fid, Bytes::new(data.clone()));
        }

        log::info!("PackageFileResolver: {} sources, {} binaries", sources.len(), binaries.len());

        Self { sources, binaries }
    }
}

impl FileResolver for PackageFileResolver {
    fn resolve_binary(&self, id: TypstFileId) -> FileResult<Cow<'_, Bytes>> {
        if let Some(b) = self.binaries.get(&id) {
            Ok(Cow::Borrowed(b))
        } else {
            Err(typst::diag::FileError::NotFound(id.vpath().as_rooted_path().into()))
        }
    }

    fn resolve_source(&self, id: TypstFileId) -> FileResult<Cow<'_, Source>> {
        if let Some(s) = self.sources.get(&id) {
            Ok(Cow::Borrowed(s))
        } else {
            Err(typst::diag::FileError::NotFound(id.vpath().as_rooted_path().into()))
        }
    }
}

impl TypstCompiler {
    pub fn new() -> Result<Self, String> { Ok(Self) }

    fn prepare_files(
        file_contents: &HashMap<String, String>,
        image_cache: &HashMap<String, String>,
        main_file: &str,
        pkg_sources: &[(PackageSpec, String, String)],
        pkg_binaries: &[(PackageSpec, String, Vec<u8>)],
    ) -> PreparedFiles {
        let mut sources = Vec::new();
        let mut binaries = Vec::new();

        for (path, content) in file_contents {
            if path == main_file { continue; }
            if path.ends_with(".typ") {
                sources.push((path.clone(), content.clone()));
            } else {
                binaries.push((path.clone(), content.as_bytes().to_vec()));
            }
        }

        for (path, b64) in image_cache {
            let data = if let Some(p) = b64.find(',') { &b64[p + 1..] } else { b64.as_str() };
            if let Ok(bytes) = STANDARD.decode(data) { binaries.push((path.clone(), bytes)); }
        }

        PreparedFiles {
            sources,
            binaries,
            pkg_sources: pkg_sources.to_vec(),
            pkg_binaries: pkg_binaries.to_vec(),
        }
    }

    fn build_engine(source: &str, main_file: &str, p: &PreparedFiles) -> TypstEngine<TypstTemplateMainFile> {
        let mut b = TypstEngine::builder()
            .main_file((main_file, source.to_owned()))
            .search_fonts_with(
                TypstKitFontOptions::default()
                    .include_system_fonts(false)
                    .include_embedded_fonts(true),
            );

        // Project sources
        if !p.sources.is_empty() {
            let refs: Vec<(&str, &str)> = p.sources.iter()
                .map(|(n, c)| (n.as_str(), c.as_str())).collect();
            b = b.with_static_source_file_resolver(refs);
        }

        // Project binaries
        if !p.binaries.is_empty() {
            let refs: Vec<(&str, &[u8])> = p.binaries.iter()
                .map(|(n, d)| (n.as_str(), d.as_slice())).collect();
            b = b.with_static_file_resolver(refs);
        }

        // Package files: use a single custom resolver for both sources and binaries
        if !p.pkg_sources.is_empty() || !p.pkg_binaries.is_empty() {
            let resolver = PackageFileResolver::new(&p.pkg_sources, &p.pkg_binaries);
            b = b.add_file_resolver(resolver);
        }

        b.build()
    }

    fn format_errors(source_text: &str, err: &TypstAsLibError) -> String {
        match err {
            TypstAsLibError::TypstSource(diagnostics) => {
                let source = Source::detached(source_text);
                diagnostics.iter().map(|diag| {
                    let loc = source.range(diag.span)
                        .and_then(|r| source.lines().byte_to_line_column(r.start))
                        .map(|(l, c)| format!("{}:{}", l + 1, c + 1));
                    let sev = match diag.severity {
                        typst::diag::Severity::Error => "error",
                        typst::diag::Severity::Warning => "warning",
                    };
                    let mut msg = if let Some(l) = loc { format!("{} {}: {}", l, sev, diag.message) }
                        else { format!("{}: {}", sev, diag.message) };
                    for hint in &diag.hints { msg.push_str(&format!("\n  hint: {}", hint)); }
                    msg
                }).collect::<Vec<_>>().join("\n")
            }
            other => format!("{}", other),
        }
    }

    /// Compile once, produce both SVG and PDF from the same document.
    /// This avoids parsing + compiling the entire document twice.
    pub fn compile_to_both(&self, source: &str, main_file: &str,
        fc: &HashMap<String, String>, ic: &HashMap<String, String>,
        ps: &[(PackageSpec, String, String)], pb: &[(PackageSpec, String, Vec<u8>)],
    ) -> Result<(String, Vec<u8>), String> {
        if source.trim().is_empty() { return Err("Source code is empty".to_string()); }
        let p = Self::prepare_files(fc, ic, main_file, ps, pb);
        let engine = Self::build_engine(source, main_file, &p);
        match engine.compile::<typst::layout::PagedDocument>().output {
            Ok(doc) => {
                // Generate SVG
                let mut svg = String::new();
                for (i, page) in doc.pages.iter().enumerate() {
                    if i > 0 { svg.push_str("<div style=\"margin-top:10px;border-top:1px solid #ccc;padding-top:10px;\">"); }
                    else { svg.push_str("<div>"); }
                    svg.push_str(&typst_svg::svg(page));
                    svg.push_str("</div>");
                }
                // Generate PDF from same compiled doc
                let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
                    .map_err(|e| format!("PDF error: {:?}", e))?;
                Ok((svg, pdf))
            }
            Err(e) => Err(Self::format_errors(source, &e)),
        }
    }

    pub fn compile_to_svg(&self, source: &str, main_file: &str,
        fc: &HashMap<String, String>, ic: &HashMap<String, String>,
        ps: &[(PackageSpec, String, String)], pb: &[(PackageSpec, String, Vec<u8>)],
    ) -> Result<String, String> {
        if source.trim().is_empty() { return Err("Source code is empty".to_string()); }
        let p = Self::prepare_files(fc, ic, main_file, ps, pb);
        let engine = Self::build_engine(source, main_file, &p);
        match engine.compile::<typst::layout::PagedDocument>().output {
            Ok(doc) => {
                let mut svg = String::new();
                for (i, page) in doc.pages.iter().enumerate() {
                    if i > 0 { svg.push_str("<div style=\"margin-top:10px;border-top:1px solid #ccc;padding-top:10px;\">"); }
                    else { svg.push_str("<div>"); }
                    svg.push_str(&typst_svg::svg(page));
                    svg.push_str("</div>");
                }
                Ok(svg)
            }
            Err(e) => Err(Self::format_errors(source, &e)),
        }
    }

    pub fn compile_to_pdf(&self, source: &str, main_file: &str,
        fc: &HashMap<String, String>, ic: &HashMap<String, String>,
        ps: &[(PackageSpec, String, String)], pb: &[(PackageSpec, String, Vec<u8>)],
    ) -> Result<Vec<u8>, String> {
        if source.trim().is_empty() { return Err("Source code is empty".to_string()); }
        let p = Self::prepare_files(fc, ic, main_file, ps, pb);
        let engine = Self::build_engine(source, main_file, &p);
        match engine.compile::<typst::layout::PagedDocument>().output {
            Ok(doc) => typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
                .map_err(|e| format!("PDF error: {:?}", e)),
            Err(e) => Err(Self::format_errors(source, &e)),
        }
    }
}

impl Default for TypstCompiler {
    fn default() -> Self { Self::new().expect("Failed") }
}
