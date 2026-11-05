use std::env;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use supplier_inn_kpp::{list_files,process_file,to_json_line,Detection};

fn help(){
    println!("supplier-inn-kpp — извлечение ИНН/КПП поставщика из DOC/DOCX");
    println!("Использование: supplier-inn-kpp <файл.doc | папка> [--json] [--strict]");
    println!("   --json    вывод JSON Lines (по одному объекту на каждый документ)");
    println!("   --strict  код возврата 1, если хотя бы один файл не распознан");
    println!("Требуется установленный LibreOffice Writer; исходные DOC не меняются.");
}
fn run()->ExitCode {
    let mut input:Option<String>=None;
    let mut json=false;
    let mut strict=false;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--help"|"-h"=>{help();return ExitCode::SUCCESS},
            "--json"=>json=true,
            "--strict"=>strict=true,
            _ if arg.starts_with('-') => {eprintln!("Неизвестный параметр: {arg}");help();return ExitCode::from(2)},
            _=>if input.replace(arg).is_some() {eprintln!("Укажите только один файл или каталог");return ExitCode::from(2)},
        }
    }
    let Some(path)=input else {help();return ExitCode::from(2)};
    let files=match list_files(Path::new(&path)) {
        Ok(files) if !files.is_empty()=>files,
        Ok(_)=>{eprintln!("DOC/DOCX файлы не найдены");return ExitCode::from(2)},
        Err(err)=>{eprintln!("{err}");return ExitCode::from(2)},
    };
    let total=files.len();
    let mut matched=0;
    let mut incomplete=0;
    let mut errors=0;
    for file in files {
        let result=process_file(&file);
        if json {println!("{}",to_json_line(&result));}
        else {
            match (&result.detection,&result.error) {
                (Some(Detection::Found(r)),_)=>println!("[OK] {} | Поставщик ИНН={} КПП={}",file.display(),r.inn,r.kpp.as_deref().unwrap_or("не предусмотрен (ИП)")),
                (Some(Detection::NotFound),_)=>println!("[НЕТ РЕКВИЗИТОВ] {}",file.display()),
                (Some(Detection::Ambiguous),_)=>println!("[ПРОВЕРИТЬ ВРУЧНУЮ] {} | Неоднозначные или некорректные реквизиты",file.display()),
                (_,Some(e))=>eprintln!("[ОШИБКА] {} | {}",file.display(),e),
                _=>eprintln!("[ОШИБКА] {}",file.display()),
            }
        }
        match result.detection {
            Some(Detection::Found(_))=>matched+=1,
            Some(_)=>incomplete+=1,
            None=>errors+=1,
        }
    }
    if !json {println!("ИТОГО: файлов {total}, найдено {matched}, не подтверждено {incomplete}, ошибки чтения {errors}");}
    if errors>0 || (strict && incomplete>0) {ExitCode::FAILURE} else {ExitCode::SUCCESS}
}


// UTC timestamps are generated without external crates; this also works on Windows.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m as u32, d as u32)
}

fn utc_timestamp(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = duration.as_secs();
    let days = (seconds / 86_400) as i64;
    let second_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        second_of_day / 3_600,
        (second_of_day / 60) % 60,
        second_of_day % 60,
        duration.subsec_millis()
    )
}

fn elapsed_hms(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60,
        duration.subsec_millis()
    )
}

fn main() -> ExitCode {
    // Instant is monotonic: elapsed time remains reliable if the system clock changes.
    let started = Instant::now();
    eprintln!("[TIME] Измерение начато (UTC): {}", utc_timestamp(SystemTime::now()));

    // All exit paths, including --help and invalid arguments, report total runtime.
    let exit_code = run();

    let elapsed = started.elapsed();
    eprintln!("[TIME] Измерение завершено (UTC): {}", utc_timestamp(SystemTime::now()));
    eprintln!(
        "[TIME] Time elapsed (main): {} ({:.3} s)",
        elapsed_hms(elapsed),
        elapsed.as_secs_f64()
    );
    exit_code
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn iso_utc_epoch_and_leap_year() {
        assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_secs(946_684_800)),
            "2000-01-01T00:00:00.000Z"
        );
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_secs(1_793_577_600) + Duration::from_millis(123)),
            "2026-11-02T00:00:00.123Z"
        );
    }

    #[test]
    fn time_elapsed_formats_fractional_seconds_and_long_runs() {
        assert_eq!(elapsed_hms(Duration::from_millis(1_234)), "00:00:01.234");
        assert_eq!(elapsed_hms(Duration::from_secs(3_661)), "01:01:01.000");
    }
}
