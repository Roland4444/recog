use std::path::Path;
use supplier_inn_kpp::{process_file,Detection};

#[test]
fn actual_supplier_doc_extracts_correct_counterparty() {
    let Ok(filename) = std::env::var("SUPPLIER_DOC_TEST_FILE") else {
        eprintln!("SKIP: supply SUPPLIER_DOC_TEST_FILE to test a real DOC or DOCX");
        return;
    };
    let result=process_file(Path::new(&filename));
    if let Some(err)=result.error {panic!("DOC conversion error: {err}");}
    match result.detection.expect("detection missing") {
        Detection::Found(values)=>{
            assert_eq!(values.inn,"3025035441");
            assert_eq!(values.kpp.as_deref(),Some("302501001"));
        }
        other=>panic!("expected supplier INN/KPP from real DOC, got {other:?}"),
    }
}

#[test]
fn actual_ip_docx_extracts_inn_without_kpp() {
    let Ok(filename) = std::env::var("SUPPLIER_IP_DOCX_TEST_FILE") else {
        eprintln!("SKIP: set SUPPLIER_IP_DOCX_TEST_FILE to run on real ИП DOCX");
        return;
    };
    let result=process_file(Path::new(&filename));
    if let Some(err)=result.error {panic!("DOCX conversion error: {err}");}
    match result.detection.expect("detection missing") {
        Detection::Found(values)=>{
            assert_eq!(values.inn,"402709591781");
            assert_eq!(values.kpp,None);
        }
        other=>panic!("expected ИП INN without KPP from real DOCX, got {other:?}"),
    }
}
