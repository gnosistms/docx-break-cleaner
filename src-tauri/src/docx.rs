use crate::rules::{classify_boundary, Confidence};
use quick_xml::events::Event;
use quick_xml::{Reader, Writer};
use serde::Serialize;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use xmltree::{Element, XMLNode};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const DOCUMENT_PART: &str = "word/document.xml";
const MAX_DOCX_BYTES: u64 = 100 * 1024 * 1024;
const MAX_XML_BYTES: usize = 30 * 1024 * 1024;
pub const SAVE_CANCELLED: &str = "Save cancelled.";

#[derive(Clone, Debug, Default)]
pub(crate) struct ParagraphFormat {
    pub(crate) style: Option<String>,
    pub(crate) left_indent: Option<f64>,
    pub(crate) first_line_indent: Option<f64>,
    pub(crate) is_list: bool,
    pub(crate) is_heading: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ParagraphRecord {
    pub(crate) number: usize,
    pub(crate) body_slot: usize,
    pub(crate) text: String,
    pub(crate) format: ParagraphFormat,
    pub(crate) unsafe_content: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakCandidate {
    pub id: String,
    pub first_paragraph: usize,
    pub second_paragraph: usize,
    pub confidence: String,
    pub suggested_merge: bool,
    pub reason_code: String,
    pub reason: String,
    pub before_text: String,
    pub after_text: String,
    pub joined_text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub input_path: String,
    pub file_name: String,
    pub default_output_path: String,
    pub paragraph_count: usize,
    pub certain_count: usize,
    pub review_count: usize,
    pub excluded_complex_boundaries: usize,
    pub candidates: Vec<BreakCandidate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairResult {
    pub output_path: String,
    pub merged_count: usize,
    pub output_bytes: u64,
}

struct PackageEntry {
    name: String,
    data: Vec<u8>,
    compression: CompressionMethod,
    unix_mode: Option<u32>,
    is_dir: bool,
}

pub fn scan_docx_path(path: &str) -> Result<ScanResult, String> {
    let input = validated_docx_path(path)?;
    let entries = read_package(&input)?;
    let document_bytes = package_document_xml(&entries)?;
    let root = parse_document_xml(document_bytes)?;
    let paragraphs = collect_body_paragraphs(&root)?;
    let (candidates, excluded_complex_boundaries) = detect_candidates(&paragraphs);
    let certain_count = candidates
        .iter()
        .filter(|candidate| candidate.confidence == "certain")
        .count();
    let review_count = candidates.len().saturating_sub(certain_count);
    let file_name = input
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("document.docx")
        .to_string();
    Ok(ScanResult {
        input_path: input.to_string_lossy().to_string(),
        file_name,
        default_output_path: default_output_path(&input).to_string_lossy().to_string(),
        paragraph_count: paragraphs.len(),
        certain_count,
        review_count,
        excluded_complex_boundaries,
        candidates,
    })
}

#[cfg(test)]
pub fn repair_docx_path(
    input_path: &str,
    output_path: &str,
    candidate_ids: &[String],
) -> Result<RepairResult, String> {
    let cancelled = AtomicBool::new(false);
    repair_docx_path_with_progress(
        input_path,
        output_path,
        candidate_ids,
        &cancelled,
        |_, _| {},
    )
}

pub fn repair_docx_path_with_progress<F>(
    input_path: &str,
    output_path: &str,
    candidate_ids: &[String],
    cancelled: &AtomicBool,
    progress: F,
) -> Result<RepairResult, String>
where
    F: Fn(u8, &str),
{
    progress(3, "Preparing cleaned copy…");
    ensure_not_cancelled(cancelled)?;
    if candidate_ids.is_empty() {
        return Err("Select at least one paragraph break to repair.".to_string());
    }
    let input = validated_docx_path(input_path)?;
    let output = PathBuf::from(output_path.trim());
    validate_output_path(&input, &output)?;
    progress(8, "Reading source document…");
    let mut entries = read_package(&input)?;
    ensure_not_cancelled(cancelled)?;
    progress(24, "Checking selected repairs…");
    let document_index = entries
        .iter()
        .position(|entry| entry.name == DOCUMENT_PART)
        .ok_or_else(|| "The DOCX file is missing word/document.xml.".to_string())?;
    let root = parse_document_xml(&entries[document_index].data)?;
    let paragraphs = collect_body_paragraphs(&root)?;
    let (candidates, _) = detect_candidates(&paragraphs);
    let mut selected = Vec::new();
    for id in candidate_ids {
        let candidate = candidates
            .iter()
            .find(|candidate| &candidate.id == id)
            .ok_or_else(|| format!("The selected repair '{id}' is no longer valid."))?;
        selected.push(candidate.first_paragraph);
    }
    selected.sort_unstable();
    selected.dedup();
    ensure_not_cancelled(cancelled)?;
    progress(42, "Merging paragraph breaks…");
    entries[document_index].data = merge_document_xml(&entries[document_index].data, &selected)?;
    ensure_not_cancelled(cancelled)?;

    let parent = output
        .parent()
        .ok_or_else(|| "Choose an output location with a parent folder.".to_string())?;
    if !parent.exists() {
        return Err("The selected output folder does not exist.".to_string());
    }
    let temp = unique_temp_path(&output)?;
    if let Err(error) =
        write_package_with_progress(&temp, &entries, cancelled, &progress).and_then(|_| {
            ensure_not_cancelled(cancelled)?;
            progress(94, "Validating cleaned document…");
            validate_written_docx(&temp)
        })
    {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = ensure_not_cancelled(cancelled) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    progress(98, "Finishing cleaned copy…");
    fs::rename(&temp, &output)
        .map_err(|error| format!("Could not finalize the cleaned DOCX: {error}"))?;
    let output_bytes = fs::metadata(&output)
        .map_err(|error| format!("Could not inspect the cleaned DOCX: {error}"))?
        .len();
    progress(100, "Cleaned copy saved.");
    Ok(RepairResult {
        output_path: output.to_string_lossy().to_string(),
        merged_count: selected.len(),
        output_bytes,
    })
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err(SAVE_CANCELLED.to_string())
    } else {
        Ok(())
    }
}

fn validated_docx_path(path: &str) -> Result<PathBuf, String> {
    let value = path.trim();
    if value.is_empty() {
        return Err("Choose a DOCX file.".to_string());
    }
    let path = PathBuf::from(value);
    if path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        != Some("docx".to_string())
    {
        return Err("Only DOCX files are supported.".to_string());
    }
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("Could not inspect the selected DOCX: {error}"))?;
    if !metadata.is_file() {
        return Err("The selected path is not a file.".to_string());
    }
    if metadata.len() > MAX_DOCX_BYTES {
        return Err("The selected DOCX is larger than the 100 MB safety limit.".to_string());
    }
    Ok(path)
}

fn validate_output_path(input: &Path, output: &Path) -> Result<(), String> {
    if output
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        != Some("docx".to_string())
    {
        return Err("The cleaned copy must use the .docx extension.".to_string());
    }
    let input_absolute = fs::canonicalize(input)
        .map_err(|error| format!("Could not resolve the input path: {error}"))?;
    if output.exists() {
        let output_absolute = fs::canonicalize(output)
            .map_err(|error| format!("Could not resolve the output path: {error}"))?;
        if output_absolute == input_absolute {
            return Err("The original DOCX can never be overwritten.".to_string());
        }
        return Err("The output file already exists. Choose a new file name.".to_string());
    }
    Ok(())
}

fn default_output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    input.with_file_name(format!("{stem}.cleaned.docx"))
}

fn read_package(path: &Path) -> Result<Vec<PackageEntry>, String> {
    let file = File::open(path).map_err(|error| format!("Could not open the DOCX: {error}"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("Could not read the DOCX package: {error}"))?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("Could not inspect DOCX entry {index}: {error}"))?;
        if file.encrypted() {
            return Err("Password-protected DOCX files are not supported.".to_string());
        }
        let name = file.name().to_string();
        if file.enclosed_name().is_none() {
            return Err("The DOCX contains an unsafe package path.".to_string());
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|error| format!("Could not read '{name}' from the DOCX: {error}"))?;
        entries.push(PackageEntry {
            name,
            data,
            compression: file.compression(),
            unix_mode: file.unix_mode(),
            is_dir: file.is_dir(),
        });
    }
    Ok(entries)
}

fn package_document_xml(entries: &[PackageEntry]) -> Result<&[u8], String> {
    entries
        .iter()
        .find(|entry| entry.name == DOCUMENT_PART)
        .map(|entry| entry.data.as_slice())
        .ok_or_else(|| "The DOCX file is missing word/document.xml.".to_string())
}

fn parse_document_xml(bytes: &[u8]) -> Result<Element, String> {
    if bytes.len() > MAX_XML_BYTES {
        return Err("word/document.xml exceeds the 30 MB safety limit.".to_string());
    }
    Element::parse(Cursor::new(bytes))
        .map_err(|error| format!("Could not parse word/document.xml: {error}"))
}

fn collect_body_paragraphs(root: &Element) -> Result<Vec<ParagraphRecord>, String> {
    let body = find_element(root, "body")
        .ok_or_else(|| "The DOCX document does not contain a body.".to_string())?;
    let mut paragraphs = Vec::new();
    let mut body_element_order = 0usize;
    for node in &body.children {
        let XMLNode::Element(element) = node else {
            continue;
        };
        let body_slot = body_element_order;
        body_element_order += 1;
        if local_name(&element.name) != "p" {
            continue;
        }
        let number = paragraphs.len() + 1;
        paragraphs.push(ParagraphRecord {
            number,
            body_slot,
            text: paragraph_text(element),
            format: paragraph_format(element),
            unsafe_content: paragraph_has_unsafe_content(element),
        });
    }
    Ok(paragraphs)
}

fn detect_candidates(paragraphs: &[ParagraphRecord]) -> (Vec<BreakCandidate>, usize) {
    let mut candidates = Vec::new();
    let mut excluded_complex = 0usize;
    for pair in paragraphs.windows(2) {
        let previous = &pair[0];
        let following = &pair[1];
        if !body_slots_are_adjacent(previous.body_slot, following.body_slot) {
            continue;
        }
        if previous.unsafe_content || following.unsafe_content {
            if !previous.text.trim().is_empty() && !following.text.trim().is_empty() {
                excluded_complex += 1;
            }
            continue;
        }
        let Some(rule_match) = classify_boundary(previous, following) else {
            continue;
        };
        let before_text = tail_chars(previous.text.trim(), 130);
        let after_text = head_chars(following.text.trim(), 130);
        let joined_text = format!("{before_text}{after_text}");
        candidates.push(BreakCandidate {
            id: format!("p{}-p{}", previous.number, following.number),
            first_paragraph: previous.number,
            second_paragraph: following.number,
            confidence: match rule_match.confidence {
                Confidence::Certain => "certain",
                Confidence::Review => "review",
            }
            .to_string(),
            suggested_merge: rule_match.suggested_merge,
            reason_code: rule_match.code.to_string(),
            reason: rule_match.reason.to_string(),
            before_text,
            after_text,
            joined_text,
        });
    }
    (candidates, excluded_complex)
}

fn body_slots_are_adjacent(left: usize, right: usize) -> bool {
    right == left + 1
}

fn paragraph_text(paragraph: &Element) -> String {
    let mut value = String::new();
    append_element_text(paragraph, &mut value);
    value
}

fn append_element_text(element: &Element, output: &mut String) {
    match local_name(&element.name) {
        "tab" => output.push('\t'),
        "br" => output.push('\n'),
        _ => {}
    }
    for child in &element.children {
        match child {
            XMLNode::Element(element) => append_element_text(element, output),
            XMLNode::Text(text) if local_name(&element.name) == "t" => output.push_str(text),
            _ => {}
        }
    }
}

fn paragraph_format(paragraph: &Element) -> ParagraphFormat {
    let Some(properties) = child_element(paragraph, "pPr") else {
        return ParagraphFormat::default();
    };
    let style = child_element(properties, "pStyle").and_then(|element| attribute(element, "val"));
    let style_lower = style.as_deref().unwrap_or("").to_ascii_lowercase();
    let indent = child_element(properties, "ind");
    let left_indent = indent
        .and_then(|element| attribute(element, "left"))
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| value / 20.0);
    let first_line_indent = indent
        .and_then(|element| attribute(element, "firstLine"))
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| value / 20.0)
        .or_else(|| {
            indent
                .and_then(|element| attribute(element, "hanging"))
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| -(value / 20.0))
        });
    ParagraphFormat {
        style,
        left_indent,
        first_line_indent,
        is_list: child_element(properties, "numPr").is_some(),
        is_heading: style_lower.contains("heading")
            || style_lower.contains("title")
            || style_lower.starts_with("toc")
            || style_lower.contains("caption"),
    }
}

fn paragraph_has_unsafe_content(paragraph: &Element) -> bool {
    const UNSAFE: &[&str] = &[
        "ins",
        "del",
        "moveFrom",
        "moveTo",
        "commentRangeStart",
        "commentRangeEnd",
        "commentReference",
        "bookmarkStart",
        "bookmarkEnd",
        "sdt",
        "drawing",
        "pict",
        "object",
        "sectPr",
    ];
    element_contains_any(paragraph, UNSAFE)
}

fn element_contains_any(element: &Element, names: &[&str]) -> bool {
    if names.contains(&local_name(&element.name)) {
        return true;
    }
    element.children.iter().any(|child| match child {
        XMLNode::Element(element) => element_contains_any(element, names),
        _ => false,
    })
}

fn merge_document_xml(
    bytes: &[u8],
    selected_first_paragraphs: &[usize],
) -> Result<Vec<u8>, String> {
    let selected: HashSet<usize> = selected_first_paragraphs.iter().copied().collect();
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(bytes.len()));
    let mut depth = 0usize;
    let mut body_child_depth = None;
    let mut paragraph_number = 0usize;
    let mut continuation_paragraph = false;
    let mut skipped_properties_depth = None;

    loop {
        let event = reader
            .read_event()
            .map_err(|error| format!("Could not transform the repaired Word XML: {error}"))?;
        match event {
            Event::Start(start) => {
                let name = start.name();
                let local = local_xml_name(name.as_ref());
                if skipped_properties_depth.is_some() {
                    depth += 1;
                    continue;
                }
                if local == b"body" && body_child_depth.is_none() {
                    body_child_depth = Some(depth + 1);
                }
                if body_child_depth == Some(depth) && local == b"p" {
                    paragraph_number += 1;
                    continuation_paragraph = selected.contains(&paragraph_number.saturating_sub(1));
                    if continuation_paragraph {
                        depth += 1;
                        continue;
                    }
                } else if continuation_paragraph
                    && body_child_depth
                        .map(|value| depth == value + 1)
                        .unwrap_or(false)
                    && local == b"pPr"
                {
                    skipped_properties_depth = Some(depth);
                    depth += 1;
                    continue;
                }
                writer
                    .write_event(Event::Start(start.into_owned()))
                    .map_err(|error| format!("Could not write the repaired Word XML: {error}"))?;
                depth += 1;
            }
            Event::Empty(empty) => {
                if skipped_properties_depth.is_some() {
                    continue;
                }
                let name = empty.name();
                let local = local_xml_name(name.as_ref());
                if continuation_paragraph
                    && body_child_depth
                        .map(|value| depth == value + 1)
                        .unwrap_or(false)
                    && local == b"pPr"
                {
                    continue;
                }
                writer
                    .write_event(Event::Empty(empty.into_owned()))
                    .map_err(|error| format!("Could not write the repaired Word XML: {error}"))?;
            }
            Event::End(end) => {
                depth = depth.saturating_sub(1);
                if let Some(skipped_depth) = skipped_properties_depth {
                    if depth == skipped_depth {
                        skipped_properties_depth = None;
                    }
                    continue;
                }
                let name = end.name();
                let local = local_xml_name(name.as_ref());
                let closes_body = local == b"body" && body_child_depth == Some(depth + 1);
                if body_child_depth == Some(depth) && local == b"p" {
                    // A continuation paragraph's closing tag closes the merged paragraph.
                    // Omit it only when this paragraph also continues into the next one.
                    let omit_end = selected.contains(&paragraph_number);
                    continuation_paragraph = false;
                    if omit_end {
                        continue;
                    }
                }
                writer
                    .write_event(Event::End(end.into_owned()))
                    .map_err(|error| format!("Could not write the repaired Word XML: {error}"))?;
                if closes_body {
                    body_child_depth = None;
                }
            }
            Event::Eof => break,
            other => {
                if skipped_properties_depth.is_none() {
                    writer.write_event(other.into_owned()).map_err(|error| {
                        format!("Could not write the repaired Word XML: {error}")
                    })?;
                }
            }
        }
    }
    Ok(writer.into_inner())
}

fn local_xml_name(name: &[u8]) -> &[u8] {
    name.rsplit(|value| *value == b':').next().unwrap_or(name)
}

fn write_package_with_progress<F>(
    path: &Path,
    entries: &[PackageEntry],
    cancelled: &AtomicBool,
    progress: &F,
) -> Result<(), String>
where
    F: Fn(u8, &str),
{
    let file = File::create(path)
        .map_err(|error| format!("Could not create the cleaned DOCX: {error}"))?;
    let mut writer = ZipWriter::new(file);
    let total_bytes = entries
        .iter()
        .map(|entry| entry.data.len() as u64)
        .sum::<u64>()
        .max(1);
    let mut written_bytes = 0u64;
    progress(50, "Writing cleaned document…");
    for entry in entries {
        ensure_not_cancelled(cancelled)?;
        let mut options = SimpleFileOptions::default().compression_method(entry.compression);
        if let Some(mode) = entry.unix_mode {
            options = options.unix_permissions(mode);
        }
        if entry.is_dir {
            writer
                .add_directory(&entry.name, options)
                .map_err(|error| format!("Could not write '{}': {error}", entry.name))?;
        } else {
            writer
                .start_file(&entry.name, options)
                .map_err(|error| format!("Could not write '{}': {error}", entry.name))?;
            for chunk in entry.data.chunks(64 * 1024) {
                ensure_not_cancelled(cancelled)?;
                writer
                    .write_all(chunk)
                    .map_err(|error| format!("Could not write '{}': {error}", entry.name))?;
                written_bytes += chunk.len() as u64;
                let percent = 50 + ((written_bytes * 40 / total_bytes).min(40) as u8);
                progress(percent, "Writing cleaned document…");
            }
        }
    }
    writer
        .finish()
        .map_err(|error| format!("Could not finalize the cleaned DOCX package: {error}"))?;
    Ok(())
}

fn validate_written_docx(path: &Path) -> Result<(), String> {
    let file =
        File::open(path).map_err(|error| format!("Could not reopen the cleaned DOCX: {error}"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("The cleaned DOCX package is invalid: {error}"))?;
    let mut document = archive
        .by_name(DOCUMENT_PART)
        .map_err(|error| format!("The cleaned DOCX is missing document.xml: {error}"))?;
    let mut bytes = Vec::new();
    document
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not validate the cleaned Word XML: {error}"))?;
    parse_document_xml(&bytes)?;
    Ok(())
}

fn unique_temp_path(output: &Path) -> Result<PathBuf, String> {
    for attempt in 1..=1000u16 {
        let extension = format!("docx.cleaner-{attempt}.tmp");
        let candidate = output.with_extension(extension);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Could not allocate a temporary output file.".to_string())
}

fn find_element<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    if local_name(&element.name) == name {
        return Some(element);
    }
    element.children.iter().find_map(|child| match child {
        XMLNode::Element(element) => find_element(element, name),
        _ => None,
    })
}

fn child_element<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
    element.children.iter().find_map(|child| match child {
        XMLNode::Element(element) if local_name(&element.name) == name => Some(element),
        _ => None,
    })
}

fn attribute(element: &Element, name: &str) -> Option<String> {
    element
        .attributes
        .iter()
        .find_map(|(key, value)| (local_name(key) == name).then(|| value.clone()))
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn tail_chars(value: &str, max: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    chars[chars.len().saturating_sub(max)..].iter().collect()
}

fn head_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
      <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="xml" ContentType="application/xml"/>
      </Types>"#;

    fn make_docx(path: &Path, body: &str) {
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
          <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
            xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"
            mc:Ignorable="w14">
            <w:body>{body}</w:body>
          </w:document>"#
        );
        let file = File::create(path).expect("fixture file");
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        writer
            .start_file("[Content_Types].xml", options)
            .expect("content types");
        writer
            .write_all(CONTENT_TYPES.as_bytes())
            .expect("content types bytes");
        writer
            .start_file(DOCUMENT_PART, options)
            .expect("document part");
        writer
            .write_all(document.as_bytes())
            .expect("document bytes");
        writer.finish().expect("fixture zip");
    }

    #[test]
    fn scans_and_repairs_a_verified_split() {
        let temp = tempdir().expect("temp dir");
        let input = temp.path().join("input.docx");
        let output = temp.path().join("output.docx");
        make_docx(
            &input,
            r#"<w:p w14:paraId="11111111"><w:pPr><w:ind w:left="790" w:firstLine="276"/></w:pPr><w:r><w:t>学校でプログラ</w:t></w:r></w:p>
               <w:p w14:paraId="22222222"><w:pPr><w:ind w:left="790"/></w:pPr><w:r><w:t>ムされた。</w:t></w:r></w:p>"#,
        );
        let scan = scan_docx_path(input.to_str().expect("input path")).expect("scan");
        assert_eq!(scan.certain_count, 1);
        assert_eq!(scan.review_count, 0);
        let id = scan.candidates[0].id.clone();
        let repaired = repair_docx_path(
            input.to_str().expect("input path"),
            output.to_str().expect("output path"),
            &[id],
        )
        .expect("repair");
        assert_eq!(repaired.merged_count, 1);
        let rescanned = scan_docx_path(output.to_str().expect("output path")).expect("rescan");
        assert_eq!(rescanned.paragraph_count, 1);
        assert!(rescanned.candidates.is_empty());
        let entries = read_package(&output).expect("repaired package");
        let xml = String::from_utf8(
            package_document_xml(&entries)
                .expect("document XML")
                .to_vec(),
        )
        .expect("UTF-8 document XML");
        assert!(xml.contains("mc:Ignorable=\"w14\""));
        assert!(xml.contains("w14:paraId=\"11111111\""));
        assert!(xml.contains("w:left=\"790\""));
        assert!(!xml.contains(" Ignorable=\""));
        assert!(!xml.contains(" paraId=\""));
    }

    #[test]
    fn never_overwrites_the_source() {
        let temp = tempdir().expect("temp dir");
        let input = temp.path().join("input.docx");
        make_docx(
            &input,
            r#"<w:p><w:r><w:t>問</w:t></w:r></w:p><w:p><w:r><w:t>題</w:t></w:r></w:p>"#,
        );
        let error = repair_docx_path(
            input.to_str().expect("input path"),
            input.to_str().expect("input path"),
            &["p1-p2".to_string()],
        )
        .expect_err("overwrite must fail");
        assert!(error.contains("never be overwritten"));
    }

    #[test]
    fn cancelled_repair_never_creates_an_output() {
        let temp = tempdir().expect("temp dir");
        let input = temp.path().join("input.docx");
        let output = temp.path().join("output.docx");
        make_docx(
            &input,
            r#"<w:p><w:r><w:t>プログラ</w:t></w:r></w:p><w:p><w:r><w:t>ムされた。</w:t></w:r></w:p>"#,
        );
        let cancelled = AtomicBool::new(true);
        let error = repair_docx_path_with_progress(
            input.to_str().expect("input path"),
            output.to_str().expect("output path"),
            &["p1-p2".to_string()],
            &cancelled,
            |_, _| {},
        )
        .expect_err("cancelled repair");
        assert_eq!(error, SAVE_CANCELLED);
        assert!(!output.exists());
    }

    #[test]
    fn cancellation_during_write_removes_the_temporary_file() {
        let temp = tempdir().expect("temp dir");
        let input = temp.path().join("input.docx");
        let output = temp.path().join("output.docx");
        make_docx(
            &input,
            r#"<w:p><w:r><w:t>プログラ</w:t></w:r></w:p><w:p><w:r><w:t>ムされた。</w:t></w:r></w:p>"#,
        );
        let cancelled = AtomicBool::new(false);
        let error = repair_docx_path_with_progress(
            input.to_str().expect("input path"),
            output.to_str().expect("output path"),
            &["p1-p2".to_string()],
            &cancelled,
            |percent, _| {
                if percent >= 50 {
                    cancelled.store(true, Ordering::Relaxed);
                }
            },
        )
        .expect_err("cancelled during write");
        assert_eq!(error, SAVE_CANCELLED);
        assert!(!output.exists());
        let leftovers = fs::read_dir(temp.path())
            .expect("temp directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains("cleaner-"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn reference_document_counts() {
        let Ok(path) = std::env::var("DOCX_CLEANER_REFERENCE") else {
            eprintln!("DOCX_CLEANER_REFERENCE is not set; skipping local golden assertion");
            return;
        };
        let scan = scan_docx_path(&path).expect("reference DOCX scan");
        eprintln!(
            "reference scan: {} certain, {} review, {} complex exclusions",
            scan.certain_count, scan.review_count, scan.excluded_complex_boundaries
        );
        assert_eq!(scan.certain_count, 42);
        assert_eq!(scan.review_count, 24);
        let suggested_review_merges: Vec<&BreakCandidate> = scan
            .candidates
            .iter()
            .filter(|candidate| candidate.confidence == "review" && candidate.suggested_merge)
            .collect();
        let suggested_review_keeps: Vec<&BreakCandidate> = scan
            .candidates
            .iter()
            .filter(|candidate| candidate.confidence == "review" && !candidate.suggested_merge)
            .collect();
        assert_eq!(suggested_review_merges.len(), 23);
        assert_eq!(suggested_review_keeps.len(), 1);
        assert_eq!(suggested_review_keeps[0].id, "p178-p179");
    }

    #[test]
    fn reference_document_repairs_all_certain_candidates() {
        let Ok(path) = std::env::var("DOCX_CLEANER_REFERENCE") else {
            eprintln!("DOCX_CLEANER_REFERENCE is not set; skipping local repair assertion");
            return;
        };
        let scan = scan_docx_path(&path).expect("reference DOCX scan");
        let candidate_ids: Vec<String> = scan
            .candidates
            .iter()
            .filter(|candidate| candidate.confidence == "certain")
            .map(|candidate| candidate.id.clone())
            .collect();
        let temp = tempdir().expect("repair temp dir");
        let output = std::env::var("DOCX_CLEANER_TEST_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| temp.path().join("reference-cleaned.docx"));
        let repaired = repair_docx_path(
            &path,
            output.to_str().expect("repair output path"),
            &candidate_ids,
        )
        .expect("reference DOCX repair");
        assert_eq!(repaired.merged_count, 42);
        let rescanned = scan_docx_path(output.to_str().expect("repair output path"))
            .expect("cleaned DOCX scan");
        assert_eq!(rescanned.paragraph_count, scan.paragraph_count - 42);
        assert_eq!(rescanned.certain_count, 0);
    }
}
