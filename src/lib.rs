
//! Offline recognition of Russian supplier INN/KPP in Word documents.
//! Both .doc and .docx are converted to XHTML with LibreOffice (never edited).
//! Tables are parsed cell-by-cell to avoid confusing Supplier with Buyer.

mod metadata;
pub use metadata::{DocumentInfo, InvoiceInfo, extract_document_info_from_html};

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requisites {
    pub inn: String,
    // A 12-digit INN belongs to an individual (including an ИП): no КПП is assigned.
    // A 10-digit legal-entity INN requires a valid 9-digit КПП to be complete.
    pub kpp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Found(Requisites),
    NotFound,
    Ambiguous,
}

#[derive(Debug, Clone)]
pub struct FileResult {
    pub path: PathBuf,
    pub detection: Option<Detection>,
    pub details: Option<DocumentInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct Cell {
    text: String,
    colspan: usize,
}

type Table = Vec<Vec<Cell>>;

#[derive(Debug, Default)]
struct HtmlDoc {
    tables: Vec<Table>,
    text_outside_tables: String,
}

pub fn is_word_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            e.eq_ignore_ascii_case("doc")
                || e.eq_ignore_ascii_case("docx")
        })
}

/// Returns every DOC and DOCX file under a directory, including subfolders.
/// Directory symlinks are not followed.
pub fn list_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        if !is_word_file(path) {
            return Err("Поддерживаются только .doc и .docx".into());
        }
        return Ok(vec![path.to_path_buf()]);
    }

    if !path.is_dir() {
        return Err(format!(
            "Не найден файл или каталог: {}",
            path.display()
        ));
    }

    let mut dirs = vec![path.to_path_buf()];
    let mut files = Vec::new();

    while let Some(dir) = dirs.pop() {
        for item in fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
        {
            let item = item.map_err(|e| e.to_string())?;
            let kind = item.file_type().map_err(|e| e.to_string())?;

            if kind.is_dir() {
                dirs.push(item.path());
            } else if kind.is_file() && is_word_file(&item.path()) {
                files.push(item.path());
            }
        }
    }

    files.sort();
    Ok(files)
}

struct TempWorkspace(PathBuf);

impl TempWorkspace {
    fn create() -> Result<Self, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();

        let path = std::env::temp_dir().join(format!(
            "supplier-inn-kpp-{}-{now}",
            std::process::id()
        ));

        fs::create_dir(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;

        Ok(Self(path))
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn word_to_xhtml(path: &Path) -> Result<String, String> {
    let workspace = TempWorkspace::create()?;
    let profile = workspace.0.join("lo-profile");

    let profile_uri = if cfg!(windows) {
        format!(
            "file:///{}",
            profile.to_string_lossy().replace('\\', "/")
        )
    } else {
        format!("file://{}", profile.to_string_lossy())
    };

    // On Windows, set LIBREOFFICE_BIN to the full path to soffice.exe.
    let executable = std::env::var_os("LIBREOFFICE_BIN")
        .unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            {
                std::ffi::OsString::from(
                    r"C:\Program Files\LibreOffice\program\soffice.exe"
                )
            }
            #[cfg(not(target_os = "windows"))]
            {
                std::ffi::OsString::from("libreoffice")
            }
        });

    let output = Command::new(executable)
        .arg(format!("-env:UserInstallation={profile_uri}"))
        .arg("--headless")
        .arg("--convert-to")
        .arg("html:XHTML Writer File")
        .arg("--outdir")
        .arg(&workspace.0)
        .arg(path)
        .output()
        .map_err(|e| {
            format!(
                "LibreOffice не запущен: {e}. \
                 Установите LibreOffice или задайте LIBREOFFICE_BIN"
            )
        })?;

    if !output.status.success() {
        return Err(format!(
            "LibreOffice: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stem = path
        .file_stem()
        .ok_or("Некорректное имя файла")?;

    let html_file = workspace.0.join(stem).with_extension("html");

    fs::read_to_string(&html_file).map_err(|e| {
        format!(
            "XHTML не создан для {}: {e}; stdout: {} stderr: {}",
            path.display(),
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })
}

fn html_unescape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;

    while i < input.len() {
        if input.as_bytes()[i] == b'&' {
            if let Some(end_rel) = input[i + 1..].find(';') {
                let end = i + 1 + end_rel;

                if end - i <= 12 {
                    let entity = &input[i + 1..end];

                    let replacement = match entity {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        "nbsp" => Some(' '),
                        _ => {
                            if let Some(hex) = entity
                                .strip_prefix("#x")
                                .or_else(|| entity.strip_prefix("#X"))
                            {
                                u32::from_str_radix(hex, 16)
                                    .ok()
                                    .and_then(char::from_u32)
                            } else if let Some(dec) = entity.strip_prefix('#') {
                                dec.parse::<u32>()
                                    .ok()
                                    .and_then(char::from_u32)
                            } else {
                                None
                            }
                        }
                    };

                    if let Some(ch) = replacement {
                        out.push(ch);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }

        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn append_text(
    cell: &mut Option<Cell>,
    outside: &mut String,
    text: &str,
) {
    if let Some(current) = cell {
        current.text.push_str(text);
    } else {
        outside.push_str(text);
    }
}

fn clean_text(input: &str) -> String {
    let mut result = String::new();

    for line in input
        .replace('\u{a0}', " ")
        .replace('\r', "\n")
        .lines()
    {
        let line = line
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        if !line.is_empty() {
            result.push_str(&line);
            result.push('\n');
        }
    }

    result
}

fn attr_usize(tag: &str, name: &str) -> usize {
    let lower = tag.to_ascii_lowercase();

    if let Some(idx) = lower.find(name) {
        let after = tag[idx + name.len()..].trim_start();

        if let Some(after) = after.strip_prefix('=') {
            let v = after
                .trim_start()
                .trim_start_matches(|c: char| c == '"' || c == '\'');

            return v
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<usize>()
                .unwrap_or(1);
        }
    }

    1
}

/// Minimal, table-aware XHTML reader; no network or external parser needed.
fn parse_xhtml(html: &str) -> HtmlDoc {
    let mut doc = HtmlDoc::default();
    let mut current_table: Option<Table> = None;
    let mut current_row: Option<Vec<Cell>> = None;
    let mut current_cell: Option<Cell> = None;

    let mut body = false;
    let mut ignored = false;
    let mut i = 0;

    while i < html.len() {
        let Some(offset) = html[i..].find('<') else {
            if body && !ignored {
                append_text(
                    &mut current_cell,
                    &mut doc.text_outside_tables,
                    &html_unescape(&html[i..]),
                );
            }
            break;
        };

        let lt = i + offset;

        if lt > i && body && !ignored {
            append_text(
                &mut current_cell,
                &mut doc.text_outside_tables,
                &html_unescape(&html[i..lt]),
            );
        }

        if html[lt..].starts_with("<!--") {
            if let Some(end) = html[lt + 4..].find("-->") {
                i = lt + 4 + end + 3;
                continue;
            }
        }

        let Some(gt_rel) = html[lt..].find('>') else {
            break;
        };

        let gt = lt + gt_rel;
        let tag_raw = html[lt + 1..gt].trim();
        let closing = tag_raw.starts_with('/');
        let tag = tag_raw.trim_start_matches('/');

        let name = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        match (closing, name.as_str()) {
            (false, "body") => body = true,
            (true, "body") => body = false,

            (false, "script") | (false, "style") => ignored = true,
            (true, "script") | (true, "style") => ignored = false,

            (false, "table") => {
                if current_table.is_none() {
                    current_table = Some(Vec::new());
                }
            }

            (true, "table") => {
                if let Some(t) = current_table.take() {
                    doc.tables.push(t);
                }
            }

            (false, "tr") => {
                current_row = Some(Vec::new());
            }

            (true, "tr") => {
                if let (Some(table), Some(row)) =
                    (&mut current_table, current_row.take())
                {
                    table.push(row);
                }
            }

            (false, "td") | (false, "th") => {
                current_cell = Some(Cell {
                    text: String::new(),
                    colspan: attr_usize(tag_raw, "colspan"),
                });
            }

            (true, "td") | (true, "th") => {
                if let (Some(row), Some(mut c)) =
                    (&mut current_row, current_cell.take())
                {
                    c.text = clean_text(&c.text);
                    row.push(c);
                }
            }

            (false, "br")
            | (true, "p")
            | (true, "div")
            | (true, "h1")
            | (true, "h2")
            | (true, "h3") => {
                if body {
                    append_text(
                        &mut current_cell,
                        &mut doc.text_outside_tables,
                        "\n",
                    );
                }
            }

            _ => {}
        }

        i = gt + 1;
    }

    doc.text_outside_tables = clean_text(&doc.text_outside_tables);
    doc
}

fn check_inn(inn: &str) -> bool {
    let digits: Vec<u32> = inn
        .bytes()
        .filter(|c| c.is_ascii_digit())
        .map(|c| (c - b'0') as u32)
        .collect();

    if digits.len() != inn.len() {
        return false;
    }

    let check = |weights: &[u32]| -> u32 {
        weights
            .iter()
            .zip(digits.iter())
            .map(|(w, d)| w * d)
            .sum::<u32>()
            % 11
            % 10
    };

    match digits.len() {
        10 => check(&[2, 4, 10, 3, 5, 9, 4, 6, 8]) == digits[9],

        12 => {
            check(&[7, 2, 4, 10, 3, 5, 9, 4, 6, 8]) == digits[10]
                && check(&[3, 7, 2, 4, 10, 3, 5, 9, 4, 6, 8])
                    == digits[11]
        }

        _ => false,
    }
}

fn valid_label_start(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();

    !before.is_some_and(|c| c.is_alphabetic() || c.is_ascii_digit())
        && !after.is_some_and(|c| c.is_alphabetic() || c.is_ascii_digit())
}

fn labels(text: &str, word: &str) -> Vec<(usize, usize)> {
    text.match_indices(word)
        .filter_map(|(idx, s)| {
            let end = idx + s.len();

            valid_label_start(text, idx, end)
                .then_some((idx, end))
        })
        .collect()
}

fn read_digits(
    text: &str,
    from: usize,
    limit: usize,
) -> Option<(String, usize)> {
    let mut i = from;
    let mut scanned = 0;

    while i < text.len() && scanned < limit {
        let c = text[i..].chars().next()?;

        if c.is_ascii_digit() {
            break;
        }

        if c.is_alphabetic() && c != '№' {
            return None;
        }

        i += c.len_utf8();
        scanned += 1;
    }

    let start = i;

    while i < text.len() && text.as_bytes()[i].is_ascii_digit() {
        i += 1;
    }

    (i > start).then(|| (text[start..i].into(), i))
}

/// Recognizes:
///   ИНН: 3025035441 КПП: 302501001
///   ИНН/КПП 3025035441/302501001
///   ИНН 3461070140/346101001
fn parse_numbers_in_section(section: &str) -> Option<Requisites> {
    let lower = section.to_lowercase().replace('\u{a0}', " ");
    let mut matches = Vec::new();

    for (start, end) in labels(&lower, "инн") {
        let following = lower[end..].trim_start();

        let paired = following.starts_with("/кпп")
            || following.starts_with("/ кпп")
            || following.starts_with("\\кпп")
            || following.starts_with("\\ кпп");

        let number_from = if paired {
            let offset = lower[end..].find("кпп")?;
            end + offset + "кпп".len()
        } else {
            end
        };

        let Some((inn, inn_end)) =
            read_digits(&lower, number_from, 24)
        else {
            continue;
        };

        if !check_inn(&inn) {
            continue;
        }

        // Support INN/KPP written on one line:
        // ИНН 3461070140/346101001
        let after_inn = lower[inn_end..].trim_start();

        let slash_kpp = if after_inn.starts_with('/')
            || after_inn.starts_with('\\')
        {
            let separator_pos = lower.len() - after_inn.len();

            read_digits(&lower, separator_pos + 1, 8)
                .map(|(value, _)| value)
        } else {
            None
        };

        // Support separate KPP label:
        // ИНН 3000013051 КПП 300001001
        let kpp = slash_kpp.or_else(|| {
            if paired {
                return None;
            }

            labels(&lower, "кпп")
                .into_iter()
                .filter(|(kstart, _)| *kstart >= start)
                .find_map(|(_, kend)| {
                    read_digits(&lower, kend, 15)
                        .map(|(value, _)| value)
                })
        });

        if inn.len() == 12 {
            // Individual entrepreneurs do not have a KPP.
            if kpp.is_none() {
                matches.push(Requisites {
                    inn,
                    kpp: None,
                });
            }
        } else if let Some(kpp) = kpp {
            if kpp.len() == 9 {
                matches.push(Requisites {
                    inn,
                    kpp: Some(kpp),
                });
            }
        }
    }

    matches.sort_by(|a, b| {
        a.inn.cmp(&b.inn).then(a.kpp.cmp(&b.kpp))
    });
    matches.dedup();

    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

const VENDOR_LABELS: &[&str] = &[
    "поставщик",
    "продавец",
    "исполнитель",
    "подрядчик",
    "рекламораспространитель",
];

const OTHER_LABELS: &[&str] = &[
    "покупатель",
    "заказчик",
    "клиент",
    "получатель",
    "рекламодатель",
];

/// Only exact role headings trigger a match.
/// Mentions like "Поставщик обязан" are deliberately ignored.
fn role_markers(text: &str) -> Vec<(usize, usize, bool)> {
    let lower = text.to_lowercase();
    let mut found = Vec::new();

    for (words, is_vendor) in [
        (VENDOR_LABELS, true),
        (OTHER_LABELS, false),
    ] {
        for word in words {
            for (start, end) in labels(&lower, word) {
                let suffix = &lower[end..];

                let trimmed = suffix.trim_start_matches(
                    |c: char| c.is_whitespace()
                );

                let heading = trimmed.starts_with(':')
                    || trimmed.starts_with('：');

                if heading || lower[..start].ends_with("реквизиты ") {
                    found.push((start, end, is_vendor));
                }
            }
        }
    }

    found.sort_by_key(|v| v.0);
    found
}

fn candidate_sections(text: &str) -> Vec<String> {
    let roles = role_markers(text);
    let mut regions = Vec::new();

    for (idx, (_, end, is_vendor)) in roles.iter().enumerate() {
        if !is_vendor {
            continue;
        }

        let stop = roles
            .get(idx + 1)
            .map(|x| x.0)
            .unwrap_or(text.len());

        regions.push(text[*end..stop].to_string());
    }

    regions
}

/// Parses LibreOffice XHTML and prevents buyer details from leaking
/// across a two-column Word table into supplier details.
pub fn detect_supplier_in_html(html: &str) -> Detection {
    let doc = parse_xhtml(html);
    let mut candidates = Vec::new();
    let mut had_ambiguity = false;

    for table in &doc.tables {
        // Scan each individual cell.
        for row in table {
            for cell in row {
                for region in candidate_sections(&cell.text) {
                    if let Some(req) = parse_numbers_in_section(&region) {
                        candidates.push(req);
                    } else if region.to_lowercase().contains("инн") {
                        had_ambiguity = true;
                    }
                }
            }
        }

        // Scan each column across multiple rows.
        let width = table
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.colspan)
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0);

        for col in 0..width.min(32) {
            let mut column = String::new();

            for row in table {
                let mut cur = 0;

                for cell in row {
                    if col >= cur && col < cur + cell.colspan {
                        column.push_str(&cell.text);
                        column.push('\n');
                        break;
                    }

                    cur += cell.colspan;
                }
            }

            for region in candidate_sections(&column) {
                if let Some(req) = parse_numbers_in_section(&region) {
                    candidates.push(req);
                } else if region.to_lowercase().contains("инн") {
                    had_ambiguity = true;
                }
            }
        }
    }

    // Scan ordinary text outside tables.
    for region in candidate_sections(&doc.text_outside_tables) {
        if let Some(req) = parse_numbers_in_section(&region) {
            candidates.push(req);
        } else if region.to_lowercase().contains("инн") {
            had_ambiguity = true;
        }
    }

    candidates.sort_by(|a, b| {
        a.inn.cmp(&b.inn).then(a.kpp.cmp(&b.kpp))
    });

    candidates.dedup();

    if candidates.len() > 1
        || (candidates.len() == 1 && had_ambiguity)
    {
        Detection::Ambiguous
    } else if let Some(req) = candidates.pop() {
        Detection::Found(req)
    } else if had_ambiguity {
        Detection::Ambiguous
    } else {
        Detection::NotFound
    }
}

pub fn process_file(path: &Path) -> FileResult {
    match word_to_xhtml(path) {
        Ok(html) => FileResult {
            path: path.to_path_buf(),
            detection: Some(detect_supplier_in_html(&html)),
            details: Some(extract_document_info_from_html(&html)),
            error: None,
        },

        Err(error) => FileResult {
            path: path.to_path_buf(),
            detection: None,
            details: None,
            error: Some(error),
        },
    }
}

fn quote_json(input: &str) -> String {
    let mut out = String::from("\"");

    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),

            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }

            c => out.push(c),
        }
    }

    out.push('"');
    out
}

fn json_option(value: Option<&str>) -> String {
    value.map(quote_json).unwrap_or_else(|| "null".into())
}

pub fn to_json_line(result: &FileResult) -> String {
    let filename = quote_json(&result.path.display().to_string());
    let mut fields = vec![format!("\"file\":{filename}")];
    match (&result.detection, &result.error) {
        (Some(Detection::Found(r)), _) => {
            fields.push("\"status\":\"found\"".into());
            fields.push(format!("\"supplier_inn\":{}", quote_json(&r.inn)));
            fields.push(format!("\"supplier_kpp\":{}", json_option(r.kpp.as_deref())));
        }
        (Some(Detection::NotFound), _) => fields.push("\"status\":\"not_found\"".into()),
        (Some(Detection::Ambiguous), _) => fields.push("\"status\":\"ambiguous\"".into()),
        (_, Some(err)) => {
            fields.push("\"status\":\"error\"".into());
            fields.push(format!("\"message\":{}", quote_json(err)));
        }
        _ => fields.push("\"status\":\"error\"".into()),
    }
    let d = result.details.as_ref();
    for (key, value) in [
        ("document_number", d.and_then(|x|x.document_number.as_deref())),
        ("document_date", d.and_then(|x|x.document_date.as_deref())),
        ("counterparty_name", d.and_then(|x|x.counterparty_name.as_deref())),
        ("counterparty_inn", d.and_then(|x|x.counterparty_inn.as_deref())),
        ("counterparty_kpp", d.and_then(|x|x.counterparty_kpp.as_deref())),
        ("our_company_name", d.and_then(|x|x.our_company_name.as_deref())),
        ("our_company_inn", d.and_then(|x|x.our_company_inn.as_deref())),
        ("our_company_kpp", d.and_then(|x|x.our_company_kpp.as_deref())),
    ] {
        fields.push(format!("\"{key}\":{}",json_option(value)));
    }
    let invoice = d.map(|x|&x.invoice);
    let mut invoice_fields=Vec::new();
    for (key, value) in [
        ("issuer_inn", invoice.and_then(|x|x.issuer_inn.as_deref())),
        ("issuer_kpp", invoice.and_then(|x|x.issuer_kpp.as_deref())),
        ("issuer_company_name", invoice.and_then(|x|x.issuer_company_name.as_deref())),
        ("issuer_bank_account", invoice.and_then(|x|x.issuer_bank_account.as_deref())),
        ("recipient_inn", invoice.and_then(|x|x.recipient_inn.as_deref())),
        ("recipient_company_name", invoice.and_then(|x|x.recipient_company_name.as_deref())),
    ] {
        invoice_fields.push(format!("\"{key}\":{}",json_option(value)));
    }
    fields.push(format!("\"is_invoice\":{}",d.is_some_and(|x|x.is_invoice)));
    fields.push(format!("\"invoice\":{{{}}}",invoice_fields.join(",")));
    format!("{{{}}}",fields.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(body: &str) -> String {
        format!("<html><body>{body}</body></html>")
    }

    fn found(body: &str) -> Requisites {
        match detect_supplier_in_html(&html(body)) {
            Detection::Found(r) => r,
            value => panic!(
                "expected supplier, got {:?}",
                value
            ),
        }
    }

    #[test]
    fn belan_inn_checksum_is_correct() {
        assert!(check_inn("402709591781"));
        assert!(!check_inn("402709591780"));
        assert!(!check_inn("40270959178"));
    }

    #[test]
    fn belan_supplier_inn_in_docx_like_two_column_xhtml() {
        let document = r#"
        <html><body><table><tr>
          <td>
            <p>Исполнитель :</p>
            <p>ИП Белан Алексей Евгеньевич</p>
            <p>ИНН 402709591781</p>
            <p>ОГРНИП 316402700102613</p>
          </td>
          <td>
            <p>Заказчик :</p>
            <p>ООО СЗ &quot;РЭС-ЗВЕЗДНАЯ&quot;</p>
            <p>ИНН 3000013051</p>
            <p>КПП 300001001</p>
          </td>
        </tr></table></body></html>
        "#;

        let actual = detect_supplier_in_html(document);

        assert_eq!(
            actual,
            Detection::Found(Requisites {
                inn: "402709591781".to_string(),
                kpp: None,
            })
        );
    }

    #[test]
    fn zvezd_advertising_contract_from_real_doc_layout() {
        let document = r#"
        <table><tr>
          <td>
            <p>Рекламораспространитель:</p>
            <p>ООО «ИНДОР МАРКЕТИНГ»</p>
            <p>ИНН 3461070140/346101001</p>
            <p>ОГРН 1243400006365</p>
          </td>
          <td>
            <p>Рекламодатель:</p>
            <p>ООО «СЗ «РЭС-ЗВЕЗДНАЯ»</p>
            <p>ИНН 3000013051</p>
            <p>КПП 300001001</p>
          </td>
        </tr></table>
        "#;

        let result = found(document);

        assert_eq!(
            result,
            Requisites {
                inn: "3461070140".into(),
                kpp: Some("346101001".into()),
            }
        );
    }

    #[test]
    fn inline_slash_kpp_is_not_taken_from_buyer() {
        let document = r#"
        <table><tr>
          <td>
            Рекламораспространитель:
            <p>ИНН 3461070140/346101001</p>
          </td>
          <td>
            Рекламодатель:
            <p>ИНН 3000013051 КПП 300001001</p>
          </td>
        </tr></table>
        "#;

        let result = found(document);

        assert_eq!(result.inn, "3461070140");
        assert_eq!(
            result.kpp.as_deref(),
            Some("346101001")
        );
    }

    #[test]
    fn supplier_vs_buyer_in_adjacent_table_cells() {
        let result = found(
            "<table><tr>\
             <td>Поставщик:<p>ООО «БАЗИС»</p>\
             <p>ИНН/КПП 3025035441/ 302501001</p></td>\
             <td>Покупатель:<p>ООО СЗ «РЭС-НАХИМОВА»</p>\
             <p>ИНН: 3000011270</p>\
             <p>КПП: 300001001</p></td>\
             </tr></table>"
        );

        assert_eq!(result.inn, "3025035441");
        assert_eq!(
            result.kpp.as_deref(),
            Some("302501001")
        );
    }

    #[test]
    fn supplier_in_header_and_requisites_in_next_row() {
        let result = found(
            "<table>\
             <tr><td>Поставщик:</td><td>Покупатель:</td></tr>\
             <tr>\
             <td>ИНН 3025035441 КПП 302501001</td>\
             <td>ИНН 3000011270 КПП 300001001</td>\
             </tr></table>"
        );

        assert_eq!(result.inn, "3025035441");
    }

    #[test]
    fn supplier_with_separate_identifiers_in_plain_paragraphs() {
        let result = found(
            "<p>Продавец: ООО БАЗИС</p>\
             <p>ИНН: 3025035441</p>\
             <p>КПП: 302501001</p>\
             <p>Покупатель: ООО</p>\
             <p>ИНН: 3000011270 КПП: 300001001</p>"
        );

        assert_eq!(
            result.kpp.as_deref(),
            Some("302501001")
        );
    }

    #[test]
    fn docx_ip_belan_specification_in_adjacent_cells_without_kpp() {
        let result = found(
            "<table><tr>\
             <td>Исполнитель :\
             <p>ИП Белан Алексей Евгеньевич</p>\
             <p>ИНН 402709591781</p>\
             <p>ОГРНИП 316402700102613</p></td>\
             <td>Заказчик:\
             <p>ООО СЗ «РЭС-ЗВЕЗДНАЯ»</p>\
             <p>ИНН 3000013051</p>\
             <p>КПП 300001001</p></td>\
             </tr></table>"
        );

        assert_eq!(result.inn, "402709591781");
        assert_eq!(result.kpp, None);
    }

    #[test]
    fn ip_with_inn_only_in_plain_paragraphs() {
        let result = found(
            "<p>Исполнитель: ИП Белан Алексей Евгеньевич</p>\
             <p>ИНН 402709591781</p>\
             <p>Заказчик: ООО СЗ РЭС-ЗВЕЗДНАЯ</p>\
             <p>ИНН 3000013051 КПП 300001001</p>"
        );

        assert_eq!(result.inn, "402709591781");
        assert_eq!(result.kpp, None);
    }

    #[test]
    fn invalid_ip_inn_remains_ambiguous() {
        assert_eq!(
            detect_supplier_in_html(&html(
                "<p>Исполнитель: ИП</p>\
                 <p>ИНН 402709591780</p>"
            )),
            Detection::Ambiguous
        );
    }

    #[test]
    fn entity_inn_without_kpp_is_incomplete() {
        assert_eq!(
            detect_supplier_in_html(&html(
                "<p>Поставщик: ООО БАЗИС</p>\
                 <p>ИНН 3025035441</p>"
            )),
            Detection::Ambiguous
        );
    }

    #[test]
    fn ip_json_contains_null_kpp_not_buyers_kpp() {
        let result = FileResult {
            path: PathBuf::from("spec.docx"),
            detection: Some(Detection::Found(Requisites {
                inn: "402709591781".into(),
                kpp: None,
            })),
            details: None,
            error: None,
        };

        let json = to_json_line(&result);

        assert!(
            json.contains("\"supplier_inn\":\"402709591781\"")
        );
        assert!(json.contains("\"supplier_kpp\":null"));
        assert!(!json.contains("300001001"));
    }

    #[test]
    fn buyer_only_is_not_supplier() {
        assert_eq!(
            detect_supplier_in_html(&html(
                "<p>Покупатель: ООО</p>\
                 <p>ИНН: 3000011270 КПП: 300001001</p>"
            )),
            Detection::NotFound
        );
    }

    #[test]
    fn invalid_supplier_inn_flags_ambiguity() {
        assert_eq!(
            detect_supplier_in_html(&html(
                "<p>Поставщик:</p>\
                 <p>ИНН/КПП 3025035440 / 302501001</p>"
            )),
            Detection::Ambiguous
        );
    }

    #[test]
    fn contradictory_supplier_requisites_flags_ambiguity() {
        assert_eq!(
            detect_supplier_in_html(&html(
                "<p>Поставщик: ИНН/КПП 3025035441 / 302501001</p>\
                 <p>Продавец: ИНН/КПП 3000011270 / 300001001</p>"
            )),
            Detection::Ambiguous
        );
    }

    #[test]
    fn decode_html_entities() {
        assert_eq!(
            html_unescape(
                "&quot;БАЗИС&amp;Ко&quot; &#1048;&#1053;&#1053;"
            ),
            "\"БАЗИС&Ко\" ИНН"
        );
    }

    #[test]
    fn case_insensitive_extensions() {
        assert!(is_word_file(Path::new("поставщик.DOC")));
        assert!(is_word_file(Path::new("поставщик.DoCx")));
        assert!(!is_word_file(Path::new("поставщик.pdf")));
    }
}
