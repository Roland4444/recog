//! Regression test for the original problem (status: ambiguous).
//! Uses the unmodified user-supplied DOCX. Requires LibreOffice.
use std::path::Path;
use supplier_inn_kpp::{process_file, to_json_line, Detection};

#[test]
fn belan_docx_extracts_correct_inn_without_buyers_kpp() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/belan_specification_original.docx");
    assert!(path.is_file(), "missing real DOCX fixture: {}", path.display());

    let result = process_file(&path);
    assert!(result.error.is_none(), "cannot read DOCX: {:?}", result.error);

    match result.detection.as_ref() {
        Some(Detection::Found(requisites)) => {
            assert_eq!(requisites.inn, "402709591781", "must extract the ИП's INN");
        }
        actual => panic!(
            "expected Found(ИНН=402709591781) for the real DOCX, got {:?}", actual
        ),
    }

    let json = to_json_line(&result);
    assert!(json.contains("\"status\":\"found\""), "{json}");
    assert!(json.contains("\"supplier_inn\":\"402709591781\""), "{json}");
    assert!(json.contains("\"supplier_kpp\":null"), "IP must have no KPP: {json}");
    assert!(!json.contains("\"supplier_inn\":\"3000013051\""), "buyer's INN used: {json}");
    assert!(!json.contains("\"supplier_kpp\":\"300001001\""), "buyer's KPP used: {json}");
}
