//! Extraction of document header, contract parties, and invoice fields from Word XHTML.
//! Each table cell/column is processed independently to avoid assigning buyer details
//! to the supplier. Missing or contradictory evidence produces None, never guesses.
use super::{check_inn, parse_xhtml, HtmlDoc};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InvoiceInfo {
    pub issuer_inn: Option<String>,
    pub issuer_kpp: Option<String>,
    pub issuer_company_name: Option<String>,
    pub issuer_bank_account: Option<String>,
    pub recipient_inn: Option<String>,
    pub recipient_company_name: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DocumentInfo {
    pub document_number: Option<String>,
    /// ISO format YYYY-MM-DD.
    pub document_date: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_inn: Option<String>,
    pub counterparty_kpp: Option<String>,
    pub our_company_name: Option<String>,
    pub our_company_inn: Option<String>,
    pub our_company_kpp: Option<String>,
    pub is_invoice: bool,
    pub invoice: InvoiceInfo,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Party {
    name: Option<String>,
    inn: Option<String>,
    kpp: Option<String>,
    bank_account: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role { Vendor, Buyer }

const TITLES: &[&str] = &[
    "дополнительное соглашение", "универсальный передаточный документ",
    "спецификация", "счет на оплату", "счёт на оплату", "счет-фактура",
    "счёт-фактура", "договор", "накладная", "акт", "счет", "счёт", "упд",
];
const VENDOR_ROLES: &[&str] = &[
    "поставщик", "продавец", "исполнитель", "подрядчик",
    "рекламораспространитель", "грузоотправитель",
];
const BUYER_ROLES: &[&str] = &[
    "плательщик", "покупатель", "заказчик", "рекламодатель", "клиент",
];

fn invoice_heading(line: &str) -> bool {
    let s = line.trim().to_lowercase();
    ["счет на оплату", "счёт на оплату", "счет-фактура", "счёт-фактура", "счет №", "счёт №"]
        .iter().any(|x| s.starts_with(x))
}
fn title_prefix(line: &str) -> Option<(usize, &str)> {
    let s = line.to_lowercase();
    TITLES.iter().filter_map(|t| {
        s.find(t).and_then(|pos| {
            let before = s[..pos].chars().next_back();
            let after = s[pos + t.len()..].chars().next();
            if before.is_some_and(|c| c.is_alphanumeric()) || after.is_some_and(|c| c.is_alphabetic()) {
                None
            } else { Some((pos, *t)) }
        })
    }).min_by_key(|(pos, title)| (*pos, usize::MAX - title.len()))
}
fn get_header_lines(doc: &HtmlDoc) -> Vec<String> {
    // Outside text normally contains the contract header; PDFs and scans are not
    // processed in this legacy DOC/DOCX-only project.
    let mut lines: Vec<String> = doc.text_outside_tables.lines()
        .map(str::trim).filter(|s| !s.is_empty()).take(45).map(str::to_string).collect();
    if !lines.iter().any(|line| number_from_title(line).is_some()) {
        for table in doc.tables.iter().take(3) {
            for row in table.iter().take(6) {
                for cell in row {
                    for line in cell.text.lines().take(4) {
                        lines.push(line.to_string());
                    }
                }
            }
        }
    }
    lines
}

fn number_from_title(line: &str) -> Option<String> {
    let (start, title) = title_prefix(line)?;
    // A heading must appear at the beginning, not inside an arbitrary sentence.
    if !line[..start].trim().is_empty() { return None; }
    let rest = line[start + title.len()..].trim();
    let tail = if let Some(pos) = rest.find('№') {
        rest[pos + '№'.len_utf8()..].trim()
    } else {
        // "Договор 134/26" (no №), with optional "поставки" before the number.
        rest.split_whitespace().find(|part| part.chars().any(|c|c.is_ascii_digit()))?
    };
    let token = tail.trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '-');
    let number = token.chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        .collect::<String>();
    let number = number.trim_matches(|c:char|matches!(c, '/' | '-' | '_' | '.'));
    if number.is_empty() || !number.chars().any(|c|c.is_ascii_digit()) {None}
    else {Some(number.to_string())}
}

fn russian_month(token: &str) -> Option<u32> {
    match token.trim_matches(|c:char|!c.is_alphabetic()).to_lowercase().as_str() {
        "января" => Some(1), "февраля" => Some(2), "марта" => Some(3),
        "апреля" => Some(4), "мая" => Some(5), "июня" => Some(6),
        "июля" => Some(7), "августа" => Some(8), "сентября" => Some(9),
        "октября" => Some(10), "ноября" => Some(11), "декабря" => Some(12), _ => None,
    }
}
fn valid_day(y: u32, m: u32, d: u32) -> bool {
    if !(1900..=2100).contains(&y) || !(1..=12).contains(&m) || d == 0 {return false;}
    let leap = y % 400 == 0 || (y % 4 == 0 && y % 100 != 0);
    d <= match m {2 if leap=>29,2=>28,4|6|9|11=>30,_=>31}
}
fn date_in_line(line: &str) -> Option<String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for w in tokens.windows(3) {
        let d = w[0].trim_matches(|c:char|!c.is_ascii_digit()).parse::<u32>().ok();
        let m = russian_month(w[1]);
        let y = w[2].trim_matches(|c:char|!c.is_ascii_digit()).parse::<u32>().ok();
        if let (Some(d),Some(m),Some(y))=(d,m,y) {
            if valid_day(y,m,d) {return Some(format!("{y:04}-{m:02}-{d:02}"));}
        }
    }
    for token in tokens {
        let trimmed = token.trim_matches(|c:char| !c.is_ascii_digit());
        let parts: Vec<&str> = trimmed.split(['.', '-', '/']).collect();
        if parts.len()!=3 {continue;}
        if let (Ok(d),Ok(m),Ok(y))=(parts[0].parse::<u32>(),parts[1].parse::<u32>(),parts[2].parse::<u32>()) {
            if valid_day(y,m,d) {return Some(format!("{y:04}-{m:02}-{d:02}"));}
        }
    }
    None
}
fn header_info(doc: &HtmlDoc) -> (Option<String>, Option<String>, bool) {
    let lines = get_header_lines(doc);

    let heading = lines.iter().enumerate().find_map(|(i, line)| {
        number_from_title(line).map(|n| (i, n))
    });

    let number = heading.as_ref().map(|(_, n)| n.clone());

    let start = heading.as_ref().map(|(i, _)| *i).unwrap_or(0);
    let finish = (start + 12).min(lines.len());

    let mut dates: Vec<(i32, String)> = Vec::new();

    // Сначала ищем дату в обычном заголовке документа.
    for i in start..finish {
        if let Some(date) = date_in_line(&lines[i]) {
            let line = lines[i].to_lowercase();
            let mut score = 40 - (i - start) as i32;

            if line.contains("г. ") || line.starts_with("г.") {
                score += 75;
            }

            if i == start {
                score += 10;
            }

            if line.trim_start().starts_with("от ") && i < start + 3 {
                score -= 25;
            }

            dates.push((score, date));
        }
    }

    let date = dates
        .into_iter()
        .max_by_key(|(score, _)| *score)
        .map(|(_, date)| date)
        .or_else(|| {
            // Если даты нет в обычном заголовке,
            // проверяем первые таблицы документа.
            //
            // Например, в zvezd.doc:
            // Левая ячейка:  г. Волгоград
            // Правая ячейка: «03» ноября 2026 г.

            for table in doc.tables.iter().take(2) {
                for row in table.iter().take(4) {
                    for cell in row {
                        for line in cell.text.lines().take(6) {
                            if let Some(date) = date_in_line(line) {
                                return Some(date);
                            }
                        }
                    }
                }
            }

            None
        });

    let is_invoice = lines
        .iter()
        .take(12)
        .any(|s| invoice_heading(s));

    (number, date, is_invoice)
}
fn parse_role_start(line: &str, invoice: bool) -> Option<(Role, &str)> {
    let s = line.trim_start();
    let lower = s.to_lowercase();
    let mut choices: Vec<(&str,Role)> = VENDOR_ROLES.iter().map(|r|(*r,Role::Vendor))
        .chain(BUYER_ROLES.iter().map(|r|(*r,Role::Buyer))).collect();
    if invoice {choices.push(("получатель", Role::Vendor));}
    for (word,role) in choices {
        if !lower.starts_with(word) {continue;}
        let remaining=&s[word.len()..];
        let r=remaining.trim_start();
        let after = if r.starts_with('(') {
            let close=r.find(')')?;
            r[close+1..].trim_start()
        } else {r};
        if after.starts_with(':') || after.starts_with('：') {
            let colon=after.chars().next()?.len_utf8();
            return Some((role,after[colon..].trim_start()));
        }
    }
    None
}
fn role_sections(text: &str, invoice: bool) -> Vec<(Role,String)> {
    let mut found=Vec::new();
    let mut current: Option<(Role,String)> = None;
    for line in text.lines() {
        if let Some((role,after))=parse_role_start(line,invoice) {
            if let Some((r,s))=current.take() {found.push((r,s));}
            current=Some((role,format!("{after}\n")));
        } else if let Some((_,text))=&mut current {
            text.push_str(line);
            text.push('\n');
        }
    }
    if let Some(v)=current {found.push(v);}
    found
}
fn all_sections(doc: &HtmlDoc, invoice: bool) -> Vec<(Role,String)> {
    let mut out=Vec::new();
    for table in &doc.tables {
        for row in table {
            for cell in row {
                out.extend(role_sections(&cell.text,invoice));
            }
        }
        let width=table.iter().map(|row|row.iter().map(|c|c.colspan).sum::<usize>()).max().unwrap_or(0);
        for col in 0..width.min(32) {
            let mut text=String::new();
            for row in table {
                let mut pos=0;
                for cell in row {
                    if (pos..pos+cell.colspan).contains(&col) {
                        text.push_str(&cell.text);text.push('\n');break;
                    }
                    pos+=cell.colspan;
                }
            }
            out.extend(role_sections(&text,invoice));
        }
    }
    out.extend(role_sections(&doc.text_outside_tables,invoice));
    out
}

fn parse_inn(section: &str) -> Option<String> {
    let lower=section.to_lowercase();
    let mut found=Vec::new();
    for (pos,_) in lower.match_indices("инн") {
        if lower[..pos].chars().next_back().is_some_and(|c|c.is_alphanumeric()) {continue;}
        let after=&lower[pos+"инн".len()..];
        let first=after.trim_start();
        let number_part=if first.starts_with("/кпп") || first.starts_with("/ кпп") {
            first[1..].trim_start().strip_prefix("кпп")?.trim_start()
        } else {first};
        let number_part=number_part.trim_start_matches(|c:char|c==':' || c=='№' || c.is_whitespace());
        let inn=number_part.chars().take_while(|c|c.is_ascii_digit()).collect::<String>();
        if check_inn(&inn) {found.push(inn);}
    }
    found.sort();found.dedup();
    (found.len()==1).then(||found.remove(0))
}
fn parse_kpp(section: &str, inn: &Option<String>) -> Option<String> {
    if inn.as_deref().is_some_and(|s|s.len()==12) {return None;}
    let inn=inn.as_deref()?;
    let lower=section.to_lowercase();
    let mut values=Vec::new();
    // INN 3461070140/346101001, or INN/KPP 3461070140 / 346101001
    for (_,tail) in lower.match_indices(inn) {
        let _=tail;
    }
    if let Some(pos)=lower.find(inn) {
        let tail=lower[pos+inn.len()..].trim_start();
        if let Some(rest)=tail.strip_prefix('/').or_else(||tail.strip_prefix('\\')) {
            let v=rest.trim_start().chars().take_while(|c|c.is_ascii_digit()).collect::<String>();
            if v.len()==9 {values.push(v);}
        }
    }
    for (pos,_) in lower.match_indices("кпп") {
        let after=&lower[pos+"кпп".len()..];
        let after=after.trim_start_matches(|c:char|c==':' || c=='№' || c.is_whitespace());
        let first=after.chars().take_while(|c|c.is_ascii_digit()).collect::<String>();
        if first.len() == 9 {
            values.push(first.clone());
        }
        // INN/KPP may have the 10-digit INN first
        if first==inn {
            let remain=after[first.len()..].trim_start();
            if let Some(after_slash)=remain.strip_prefix('/').or_else(||remain.strip_prefix('\\')) {
                let v=after_slash.trim_start().chars().take_while(|c|c.is_ascii_digit()).collect::<String>();
                if v.len()==9 {values.push(v);}
            }
        }
    }
    values.sort();values.dedup();
    (values.len()==1).then(||values.remove(0))
}
fn field_start(line:&str)->bool {
    let s=line.trim().to_lowercase();
    ["инн", "кпп", "огрн", "юридический", "юр.", "почтовый", "фактический", "факт.",
        "банк", "бик", "р/с", "р/сч", "к/с", "корр.", "расчетный", "расчётный", "счет", "счёт",
        "адрес", "тел.", "тел:", "e-mail", "email", "директор", "генеральный", "подпись", "м.п."]
        .iter().any(|p|s.starts_with(p))
}
fn canonical_company_name(raw:&str)->Option<String> {
    let mut name=raw.split_whitespace().collect::<Vec<_>>().join(" ");
    for key in ["ИНН", "КПП", "ОГРН"] {
        if let Some(pos)=name.to_uppercase().find(key) {name.truncate(pos);}
    }
    name=name.trim().trim_matches(|c:char| c==':' || c==';' || c==',').trim().to_string();
    name=name.replace(" »", "»").replace("« ", "«").replace("- ", "-").replace(" -", "-");
    let long="общество с ограниченной ответственностью";
    if name.to_lowercase().starts_with(long) {
        let rest=name[long.len()..].trim();
        name=format!("ООО {rest}");
    }
    // Readable normalization only, not a change to the legal identity.
    if name.starts_with("ООО «СЗ «") {
        name=name.replacen("ООО «СЗ «", "ООО СЗ «", 1);
    }
    if name.starts_with("ООО «Специализированный застройщик «") {
        name=name.replacen("ООО «Специализированный застройщик «", "ООО СЗ «", 1);
    }
    if name.starts_with("ООО СЗ ") && name.ends_with("»»") {name.pop();}
    (!name.is_empty()).then_some(name)
}
fn parse_name(section:&str)->Option<String> {
    let mut lines=Vec::new();
    for line in section.lines() {
        let line=line.trim().trim_start_matches(':').trim();
        if line.is_empty(){continue;}
        if field_start(line) {break;}
        lines.push(line);
        if lines.len()==3 {break;}
    }
    let first=lines.first()?;
    let mut name=(*first).to_string();
    if lines.len()>1 && (name=="ООО" || name=="ИП" ||
            name.to_lowercase()=="общество с ограниченной ответственностью") {
        name.push(' ');name.push_str(lines[1]);
    } else if lines.len()>1 && lines[1].len()>first.len() && lines[1].starts_with(first) {
        name=lines[1].to_string();
    }
    canonical_company_name(&name)
}
fn find_account_near_marker(fragment:&str)->Option<String> {
    // bank account: exactly 20 digits, possibly grouped as "40702 810 9 0500 0022277".
    let f=fragment.trim_start_matches(|c:char|c.is_whitespace() || c==':' || c=='№');
    let mut digits=String::new();
    let mut seen=false;
    for c in f.chars().take(48) {
        if c.is_ascii_digit() {digits.push(c);seen=true;}
        else if seen && c.is_whitespace() {continue;}
        else if !seen && c.is_whitespace() {continue;}
        else {break;}
    }
    (digits.len()==20).then_some(digits)
}
fn parse_bank_account(section:&str)->Option<String> {
    let mut found=Vec::new();
    let prefixes=["р/счет", "р/счёт", "р/сч", "р/с", "расчетный счет", "расчётный счёт",
        "расчетный счёт", "расчётный счет", "счет получателя", "счёт получателя"];
    let lines:Vec<&str>=section.lines().collect();
    for (idx,line) in lines.iter().enumerate() {
        let lower=line.trim().to_lowercase();
        for marker in prefixes {
            if let Some(at)=lower.find(marker) {
                // A bank-account marker must have a word boundary.
                if lower[..at].chars().next_back().is_some_and(|c|c.is_alphabetic()){continue;}
                let after=&line[line.len()-(lower.len()-at-marker.len())..];
                let account=find_account_near_marker(after)
                    .or_else(|| lines.get(idx+1).and_then(|next|find_account_near_marker(next)));
                if let Some(account)=account {found.push(account);}
                break;
            }
        }
    }
    found.sort();found.dedup();
    (found.len()==1).then(||found.remove(0))
}
fn parse_party(section:&str)->Party {
    let inn=parse_inn(section);
    Party {name:parse_name(section), kpp:parse_kpp(section,&inn),
        bank_account:parse_bank_account(section),inn}
}
fn unique_value(values:impl Iterator<Item=String>)->Option<String> {
    let mut vals=values.collect::<Vec<_>>();vals.sort();vals.dedup();
    (vals.len()==1).then(||vals.remove(0))
}
fn combine_parties(parties:Vec<Party>)->Party {
    let inn=unique_value(parties.iter().filter_map(|p|p.inn.clone()));
    if inn.is_none() {return Party::default();}
    let matching:Vec<&Party>=parties.iter().filter(|p|p.inn==inn).collect();
    let name=matching.iter().filter_map(|p|p.name.clone()).max_by_key(|v|v.len());
    let kpp=unique_value(matching.iter().filter_map(|p|p.kpp.clone()));
    let mut account=unique_value(matching.iter().filter_map(|p|p.bank_account.clone()));
    // Some invoices have a separate "Получатель" block with the issuing company's
    // bank account but no INN. Use it only when the name matches the known issuer.
    if account.is_none() {
        if let Some(name)=&name {
            account=unique_value(parties.iter()
                .filter(|p|p.inn.is_none() && p.name.as_ref()==Some(name))
                .filter_map(|p|p.bank_account.clone()));
        }
    }
    Party {name,inn,kpp,bank_account:account}
}
pub fn extract_document_info_from_html(html:&str)->DocumentInfo {
    let doc=parse_xhtml(html);
    let (document_number,document_date,is_invoice)=header_info(&doc);
    let sections=all_sections(&doc,is_invoice);
    let vendors=sections.iter().filter(|(role,_)|*role==Role::Vendor)
        .map(|(_,text)|parse_party(text)).collect::<Vec<_>>();
    let buyers=sections.iter().filter(|(role,_)|*role==Role::Buyer)
        .map(|(_,text)|parse_party(text)).collect::<Vec<_>>();
    let vendor=combine_parties(vendors);
    let buyer=combine_parties(buyers);
    let invoice={
        InvoiceInfo {
            issuer_inn:vendor.inn.clone(),issuer_kpp:vendor.kpp.clone(),
            issuer_company_name:vendor.name.clone(),issuer_bank_account:vendor.bank_account.clone(),
            recipient_inn:buyer.inn.clone(),recipient_company_name:buyer.name.clone(),
        }
    };
    DocumentInfo {
        document_number,document_date,counterparty_name:vendor.name,
        counterparty_inn:vendor.inn,counterparty_kpp:vendor.kpp,
        our_company_name:buyer.name,our_company_inn:buyer.inn,our_company_kpp:buyer.kpp,
        is_invoice,invoice,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advertising_contract_maps_parties_without_claiming_invoice() {
        let html=r#"<html><body><p>Договор 134/26</p><p>«03» ноября 2026 г.</p>
            <table><tr><td><p>Рекламораспространитель:</p><p>ООО «ИНДОР МАРКЕТИНГ»</p>
            <p>ИНН 3461070140/346101001</p><p>Р/счет 40702810011000042401</p></td>
            <td><p>Рекламодатель:</p><p>ООО СЗ «РЭС-ЗВЕЗДНАЯ»</p>
            <p>ИНН 3000013051</p><p>КПП 300001001</p></td></tr></table></body></html>"#;
        let d=extract_document_info_from_html(html);
        assert_eq!(d.document_number.as_deref(),Some("134/26"));
        assert_eq!(d.document_date.as_deref(),Some("2026-11-03"));
        assert_eq!(d.counterparty_name.as_deref(),Some("ООО «ИНДОР МАРКЕТИНГ»"));
        assert_eq!(d.counterparty_inn.as_deref(),Some("3461070140"));
        assert_eq!(d.counterparty_kpp.as_deref(),Some("346101001"));
        assert_eq!(d.our_company_inn.as_deref(),Some("3000013051"));
        assert!(!d.is_invoice);
        assert_eq!(d.invoice.issuer_inn.as_deref(),Some("3461070140"));
        assert_eq!(d.invoice.issuer_bank_account.as_deref(),Some("40702810011000042401"));
    }
    #[test]
    fn invoice_fields_and_bank_account_stay_on_correct_side() {
        let html=r#"<html><body><p>Счёт на оплату № 1833 от 12 ноября 2026 г.</p>
          <table><tr><td><p>Поставщик (исполнитель): ООО «БАЗИС»</p>
          <p>ИНН/КПП 3025035441/302501001</p><p>Расчетный счет: 40702810011000042401</p></td>
          <td><p>Плательщик: ООО СЗ «РЭС-НАХИМОВА»</p><p>ИНН 3000011270</p>
          <p>КПП 300001001</p><p>Р/с 40702 810 9 0500 0022277</p></td></tr></table></body></html>"#;
        let d=extract_document_info_from_html(html);
        assert!(d.is_invoice);
        assert_eq!(d.document_number.as_deref(),Some("1833"));
        assert_eq!(d.document_date.as_deref(),Some("2026-11-12"));
        assert_eq!(d.invoice.issuer_inn.as_deref(),Some("3025035441"));
        assert_eq!(d.invoice.issuer_kpp.as_deref(),Some("302501001"));
        assert_eq!(d.invoice.issuer_company_name.as_deref(),Some("ООО «БАЗИС»"));
        assert_eq!(d.invoice.issuer_bank_account.as_deref(),Some("40702810011000042401"));
        assert_eq!(d.invoice.recipient_inn.as_deref(),Some("3000011270"));
        assert_eq!(d.invoice.recipient_company_name.as_deref(),Some("ООО СЗ «РЭС-НАХИМОВА»"));
    }
    #[test]
    fn ip_invoice_has_no_issuer_kpp() {
        let html=r#"<html><body><p>Счет на оплату № 8 от 02.11.2026</p>
            <p>Поставщик: ИП Белан Алексей Евгеньевич</p><p>ИНН 402709591781</p>
            <p>Р/с 40702810011000042401</p><p>Покупатель: ООО СЗ «РЭС-ЗВЕЗДНАЯ»</p>
            <p>ИНН 3000013051</p></body></html>"#;
        let d=extract_document_info_from_html(html);
        assert!(d.is_invoice);
        assert_eq!(d.invoice.issuer_kpp,None);
        assert_eq!(d.invoice.issuer_inn.as_deref(),Some("402709591781"));
        assert_eq!(d.invoice.recipient_inn.as_deref(),Some("3000013051"));
    }
}
