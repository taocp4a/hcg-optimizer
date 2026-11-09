//! Korean report text (issue #21): formal and concise for church volunteers, conclusion and
//! actions first. Same data and codes as the English text; only the wording and the order of the
//! text rendering differ.

use super::{ImageInfo, KeepReason, Report, UsedOn, format_label, human_size, reason_code};

/// Advice for a failure code.
pub(super) fn advice(code: &str) -> &'static str {
    match code {
        "unsupported_version" | "parse_failed" => {
            "ez-CGER에서 파일을 열어 다시 저장한 뒤 다시 시도하십시오."
        }
        _ => "프로그램 오류입니다. 해당 파일과 함께 문의하십시오.",
    }
}

/// Failure message for a code; `detail` is the technical (English) cause.
pub(super) fn failure_message(code: &str, detail: &str) -> String {
    let what = match code {
        "unsupported_version" => "지원하지 않는 파일 형식 버전입니다.",
        "parse_failed" => "파일을 읽을 수 없습니다. 파일이 손상되었을 수 있습니다.",
        _ => "변경 결과 검증에 실패하여 아무것도 변경하지 않았습니다.",
    };
    format!("{what} (기술 정보: {detail})")
}

pub(super) fn missing(path: &str, used: &[UsedOn]) -> String {
    format!(
        "{path}: 이미지 폴더에 없습니다({} 사용). 이 경로에 접근할 수 있는 PC에서만 표시됩니다.",
        pages(used)
    )
}

pub(super) fn transition(page: usize, kind: &str, duration_ms: i32) -> String {
    format!(
        "{page}페이지: 전환 효과가 '{}'({})로 설정되어 있습니다. 느린 PC에서는 설정보다 \
         오래 걸리므로 CUT을 권장합니다.",
        transition_label(kind),
        seconds(duration_ms)
    )
}

pub(super) fn over_size(file: &str, bytes: u64, limit: u64, shrunk: bool) -> String {
    if shrunk {
        format!(
            "{file}: 최저 화질로 저장해도 {}입니다(기준 {}).",
            human_size(bytes),
            human_size(limit)
        )
    } else {
        format!(
            "{file}: {}로 기준({})보다 크지만, 다시 저장해도 줄지 않아 그대로 두었습니다.",
            human_size(bytes),
            human_size(limit)
        )
    }
}

pub(super) fn reason_warning(reason: &KeepReason, file: &str) -> Option<(&'static str, String)> {
    let message = match reason {
        KeepReason::NotSupportedByApp(format) => format!(
            "{file}: ez-CGER가 지원하지 않는 {} 형식입니다. PNG 또는 JPG로 바꾸어 다시 넣으십시오.",
            format_label(*format)
        ),
        KeepReason::Undecodable(detail) => {
            format!("{file}: 이미지로 읽을 수 없습니다. (기술 정보: {detail})")
        }
        KeepReason::TooManyPixels { width, height, .. } => {
            format!("{file}: 해상도({width}×{height})가 너무 커서 처리하지 않고 그대로 두었습니다.")
        }
        KeepReason::EncodeFailed(detail) => {
            format!("{file}: 저장하지 못했습니다. (기술 정보: {detail})")
        }
        _ => return None,
    };
    Some((reason_code(reason), message))
}

/// "1920×1080 JPEG 화질 85 855.3 KB".
fn image_line(i: &ImageInfo) -> String {
    let size = match (i.width, i.height) {
        (Some(w), Some(h)) => format!("{w}×{h} "),
        _ => String::new(),
    };
    let quality = i.quality.map(|q| format!(" 화질 {q}")).unwrap_or_default();
    format!(
        "{size}{}{quality} {}",
        i.format.to_uppercase(),
        human_size(i.bytes)
    )
}

/// Reason a file was kept, as a short phrase.
fn reason_text(code: &str) -> &'static str {
    match code {
        "within_limits" => "기준 이내",
        "not_resized" => "변환 대상 형식 아님",
        "not_supported_by_app" => "ez-CGER 미지원 형식",
        "undecodable" => "읽을 수 없음",
        "too_many_pixels" => "해상도가 너무 큼",
        "encode_failed" => "저장 실패",
        "no_gain" => "다시 저장해도 줄지 않음",
        _ => "변경 없음",
    }
}

fn transition_label(kind: &str) -> &'static str {
    match kind {
        "mix" => "믹스",
        "left_to_right" => "왼쪽에서 오른쪽",
        "right_to_left" => "오른쪽에서 왼쪽",
        "bottom_to_top" => "아래에서 위",
        "top_to_bottom" => "위에서 아래",
        "cut" => "CUT",
        _ => "알 수 없음",
    }
}

fn seconds(ms: i32) -> String {
    format!("{}.{}초", ms / 1000, (ms % 1000) / 100)
}

fn with_warnings(text: String, warnings: usize) -> String {
    if warnings == 0 {
        text
    } else {
        format!("{text} · 확인 필요 {warnings}건")
    }
}

pub(super) fn headline(
    failed: bool,
    total: usize,
    shrunk: usize,
    before: u64,
    after: u64,
    warnings: usize,
) -> String {
    let text = if failed {
        "처리 실패".to_string()
    } else if total == 0 {
        "축소 대상 이미지 없음".to_string()
    } else if shrunk == 0 {
        format!("축소할 이미지 없음 (이미지 {total}개 확인)")
    } else {
        format!(
            "이미지 {shrunk}개 축소 ({} → {})",
            human_size(before),
            human_size(after)
        )
    };
    with_warnings(text, warnings)
}

pub(super) fn plan_headline(
    no_bundles: bool,
    nothing_to_do: bool,
    total: usize,
    to_shrink: usize,
    bytes_to_shrink: u64,
    warnings: usize,
) -> String {
    let text = if no_bundles {
        return "zip 파일 안에 .hcg 파일이 없습니다.".to_string();
    } else if nothing_to_do {
        "hcg 파일을 읽을 수 없음".to_string()
    } else if total == 0 {
        "축소 대상 이미지 없음".to_string()
    } else if to_shrink == 0 {
        format!("축소할 이미지 없음 (이미지 {total}개 확인)")
    } else {
        format!(
            "축소 대상 이미지 {to_shrink}개 ({})",
            human_size(bytes_to_shrink)
        )
    };
    with_warnings(text, warnings)
}

/// Where an image is used, as pages only: "1, 20, 38페이지", "배경", or the first few and a
/// count for long lists.
pub(super) fn pages(used: &[UsedOn]) -> String {
    const SHOWN: usize = 8;
    let background = used.iter().any(|u| u.page.is_none());
    let mut numbers: Vec<usize> = used.iter().filter_map(|u| u.page).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let mut parts = Vec::new();
    if background {
        parts.push("배경".to_string());
    }
    if !numbers.is_empty() {
        let list: Vec<String> = numbers.iter().take(SHOWN).map(|n| n.to_string()).collect();
        let more = if numbers.len() > SHOWN {
            format!(" 등 {}개 페이지", numbers.len())
        } else {
            String::new()
        };
        parts.push(format!("{}페이지{more}", list.join(", ")));
    }
    parts.join(", ")
}

/// The Korean text rendering: result and items to check first, details after.
pub(super) fn render_text(r: &Report) -> String {
    let mut out = Vec::new();
    out.push("hcg 이미지 최적화 점검 결과".to_string());
    out.push("=".repeat(28));
    out.push(format!("파일: {}", r.bundle.hcg_path));
    out.push(format!("결과: {}", r.summary.headline));

    if let Some(f) = &r.failure {
        out.push(String::new());
        out.push("[처리 실패]".into());
        out.push(format!("  {}", f.message));
        out.push(format!("  조치: {}", f.advice));
    }

    let checks: Vec<&str> = r
        .warnings
        .iter()
        .filter(|w| r.failure.as_ref().is_none_or(|f| f.code != w.code))
        .map(|w| w.message.as_str())
        .collect();
    if !checks.is_empty() {
        out.push(String::new());
        out.push(format!("[확인 필요 {}건]", checks.len()));
        for message in checks {
            out.push(format!("  - {message}"));
        }
    }

    if !r.images.is_empty() {
        out.push(String::new());
        out.push("[이미지]".into());
        for i in &r.images {
            let before = image_line(&i.before);
            let line = match (&i.after, &i.reason) {
                (Some(after), _) => format!("{before} → {}", image_line(after)),
                (None, Some(reason)) => format!("{before}, 그대로 둠 ({})", reason_text(reason)),
                (None, None) => format!("{before}, 처리하지 않음"),
            };
            out.push(format!("  {}", i.file));
            out.push(format!("    {line}"));
            if let Some(renamed) = &i.renamed_to {
                out.push(format!(
                    "    파일 이름 변경: {} (경로 {})",
                    renamed.file, renamed.path
                ));
            }
            out.push(format!("    사용: {}", pages(&i.used_on)));
        }
    }

    if !r.embedded.is_empty() {
        out.push(String::new());
        out.push("[hcg 안에 들어 있는 이미지 (변경하지 않음)]".into());
        for e in &r.embedded {
            let what = if e.kind == "embedded" {
                format!("내장 이미지 {}", human_size(e.bytes))
            } else {
                "빈 이미지".into()
            };
            out.push(format!(
                "  {what} ({})",
                pages(std::slice::from_ref(&e.used_on))
            ));
        }
    }

    if r.unreferenced_files > 0 || !r.other_files.unpaired.is_empty() {
        out.push(String::new());
        out.push("[기타 파일]".into());
        if r.unreferenced_files > 0 {
            out.push(format!(
                "  이미지 폴더에서 hcg가 사용하지 않는 파일 {}개는 그대로 복사했습니다.",
                r.unreferenced_files
            ));
        }
        for f in &r.other_files.unpaired {
            out.push(format!(
                "  {f}: 어떤 hcg 파일에도 속하지 않아 결과에 포함하지 않았습니다."
            ));
        }
    }

    out.push(String::new());
    out.push("[파일 정보]".into());
    if let Some(h) = &r.hcg {
        out.push(format!(
            "  형식 버전 0x{:X}, {}페이지, 이미지 참조 {}개",
            h.version, h.pages, h.image_refs
        ));
    }
    out.push(format!(
        "  적용 기준: 최대 {}×{}, 파일당 {}, JPEG 화질 {}부터 {}까지",
        r.policy.max_width,
        r.policy.max_height,
        human_size(r.policy.max_bytes),
        r.policy.jpeg_quality,
        r.policy.min_jpeg_quality
    ));
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Role;

    fn on(page: Option<usize>) -> UsedOn {
        UsedOn {
            page,
            layer: 1,
            object: 1,
            role: Role::Primary,
            frame: None,
        }
    }

    #[test]
    fn pages_are_listed_once_and_shortened() {
        assert_eq!(
            pages(&[on(Some(3)), on(Some(1)), on(Some(3))]),
            "1, 3페이지"
        );
        assert_eq!(pages(&[on(None), on(Some(2))]), "배경, 2페이지");
        let many: Vec<UsedOn> = (1..=14).map(|p| on(Some(p * 10))).collect();
        assert_eq!(
            pages(&many),
            "10, 20, 30, 40, 50, 60, 70, 80페이지 등 14개 페이지"
        );
    }

    #[test]
    fn headlines() {
        assert_eq!(
            headline(false, 3, 2, 30 << 20, 3 << 20, 1),
            "이미지 2개 축소 (30.0 MB → 3.0 MB) · 확인 필요 1건"
        );
        assert_eq!(
            headline(false, 0, 0, 0, 0, 6),
            "축소 대상 이미지 없음 · 확인 필요 6건"
        );
        assert_eq!(headline(true, 1, 0, 0, 0, 1), "처리 실패 · 확인 필요 1건");
        assert_eq!(
            plan_headline(false, false, 3, 2, 28 << 20, 0),
            "축소 대상 이미지 2개 (28.0 MB)"
        );
        assert_eq!(
            plan_headline(true, true, 0, 0, 0, 0),
            "zip 파일 안에 .hcg 파일이 없습니다."
        );
    }
}
