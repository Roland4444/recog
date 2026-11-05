use std::path::Path;
use supplier_inn_kpp::{process_file, Detection, to_json_line};

#[test]
fn docx_two_columns_ip_inn_without_kpp() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ip_buyer_two_columns.docx");
    let result = process_file(&fixture);
    assert!(result.error.is_none(), "DOCX read failed: {:?}", result.error);
    let json = to_json_line(&result);
    match result.detection {
        Some(Detection::Found(r)) => {
            assert_eq!(r.inn, "402709591781");
            assert_eq!(r.kpp, None);
            assert!(json.contains("\"supplier_kpp\":null"));
            assert!(!json.contains("\"supplier_kpp\":\"300001001\""));
        }
        other => panic!("expected found for ИП in DOCX, got {other:?}"),
    }
}
