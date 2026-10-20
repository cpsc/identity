//! Interface languages: English, Traditional Chinese (Taiwan), Simplified Chinese.
//!
//! Messages are a typed struct with one constant per language, so a missing translation is a
//! compile error. A runtime catalogue such as Fluent would add lookup and plural rules that a
//! few dozen fixed strings without plurals or gendered text do not need. Wording and terms follow
//! Zeabur's (`docs/wording.md` in the Zeabur repository): Traditional Chinese is written for
//! Taiwan and keeps "Passkey" in Latin letters; Simplified Chinese is written for mainland China
//! and uses 通行密钥.

/// A supported interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English.
    En,
    /// Traditional Chinese as used in Taiwan.
    ZhTw,
    /// Simplified Chinese as used in mainland China.
    ZhCn,
}

impl Lang {
    /// All languages, in the order the switcher shows them.
    pub const ALL: [Self; 3] = [Self::En, Self::ZhTw, Self::ZhCn];

    /// The BCP 47 tag.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhTw => "zh-TW",
            Self::ZhCn => "zh-CN",
        }
    }

    /// The messages.
    #[must_use]
    pub const fn messages(self) -> &'static Messages {
        match self {
            Self::En => &EN,
            Self::ZhTw => &ZH_TW,
            Self::ZhCn => &ZH_CN,
        }
    }

    /// The supported language matching one BCP 47 language range, if any.
    #[must_use]
    pub fn from_range(range: &str) -> Option<Self> {
        let range = range.trim().to_ascii_lowercase();
        let mut parts = range.split('-');
        match parts.next()? {
            "en" => Some(Self::En),
            "zh" => {
                let rest: Vec<&str> = parts.collect();
                // Script subtags decide first, then regions where Traditional script is used.
                if rest.contains(&"hant") || rest.iter().any(|r| matches!(*r, "tw" | "hk" | "mo")) {
                    Some(Self::ZhTw)
                } else {
                    Some(Self::ZhCn)
                }
            }
            _ => None,
        }
    }

    /// Chooses the language: an explicit `lang` parameter wins, then the `Accept-Language` header
    /// (highest quality first, RFC 9110 §12.5.4), then English.
    #[must_use]
    pub fn negotiate(explicit: Option<&str>, accept_language: Option<&str>) -> Self {
        if let Some(lang) = explicit.and_then(Self::from_range) {
            return lang;
        }
        let mut ranges: Vec<(u16, usize, &str)> = accept_language
            .unwrap_or_default()
            .split(',')
            .take(32)
            .enumerate()
            .filter_map(|(index, item)| {
                let mut fields = item.split(';');
                let range = fields.next()?.trim();
                let quality = fields
                    .find_map(|f| f.trim().strip_prefix("q="))
                    .map_or(Some(1000), |q| qvalue(q.trim()))?;
                (quality > 0).then_some((quality, index, range))
            })
            .collect();
        ranges.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        ranges
            .iter()
            .find_map(|(_, _, range)| Self::from_range(range))
            .unwrap_or(Self::En)
    }
}

/// Parses an RFC 9110 §12.4.2 `qvalue` into thousandths: `0`, `0.5`, `0.125`, `1`, `1.000`.
fn qvalue(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let thousandths: u16 = format!("{fraction:0<3}").parse().ok()?;
    match whole {
        "0" => Some(thousandths),
        "1" if thousandths == 0 => Some(1000),
        _ => None,
    }
}

/// Every string the sign-in pages show.
#[derive(Debug)]
#[expect(
    missing_docs,
    reason = "each field is named after where its text appears; see the English values"
)]
pub struct Messages {
    pub language_name: &'static str,
    pub nav_language: &'static str,
    pub login_title: &'static str,
    pub login_subtitle: &'static str,
    pub login_continue_to: &'static str,
    pub login_username: &'static str,
    pub login_username_hint: &'static str,
    pub login_passkey: &'static str,
    pub passkey_failed: &'static str,
    pub passkey_cancelled: &'static str,
    pub passkey_unsupported_title: &'static str,
    pub passkey_unsupported_body: &'static str,
    pub noscript_title: &'static str,
    pub noscript_body: &'static str,
    pub no_request_title: &'static str,
    pub no_request_body: &'static str,
    pub elevated_title: &'static str,
    /// `%app%` and `%minutes%` are replaced when the page is rendered.
    pub elevated_body: &'static str,
    pub working: &'static str,
    pub generic_error: &'static str,
    pub setup_title: &'static str,
    pub setup_subtitle: &'static str,
    pub setup_label: &'static str,
    pub setup_label_hint: &'static str,
    pub setup_create: &'static str,
    pub setup_done_title: &'static str,
    pub setup_done_body: &'static str,
    pub setup_invalid: &'static str,
    pub setup_missing_token: &'static str,
    pub error_title: &'static str,
    pub error_body: &'static str,
    pub error_detail_label: &'static str,
    /// `%provider%` is replaced with the provider's name, for example GitHub.
    pub continue_with: &'static str,
    pub upstream_or: &'static str,
    pub upstream_linked_title: &'static str,
    pub upstream_linked_body: &'static str,
    pub upstream_not_linked: &'static str,
    pub upstream_failed: &'static str,
    pub upstream_already_linked: &'static str,
}

/// English.
pub const EN: Messages = Messages {
    language_name: "English",
    nav_language: "Language",
    login_title: "Sign in",
    login_subtitle: "Use a passkey on this device or your phone.",
    login_continue_to: "Continue to",
    login_username: "Username",
    login_username_hint: "Optional. The browser can suggest a passkey here.",
    login_passkey: "Sign in with a passkey",
    passkey_failed: "Unable to verify the passkey. Try again or use another passkey.",
    passkey_cancelled: "The passkey dialog closed early. Try again.",
    passkey_unsupported_title: "This browser can’t use passkeys",
    passkey_unsupported_body: "Open this page in a current version of Safari, Chrome, Edge, or Firefox, or on your phone.",
    noscript_title: "JavaScript is turned off",
    noscript_body: "Passkeys need JavaScript. Turn it on for this site and reload the page.",
    no_request_title: "Nothing to sign in to",
    no_request_body: "Start from the app or command you want to use, such as kubectl. It opens this page when you need to sign in.",
    elevated_title: "Elevated access requested",
    elevated_body: "%app% is requesting elevated access for %minutes%\u{a0}min. Continue only if you started this sign-in from %app%.",
    working: "Waiting for the passkey…",
    generic_error: "Unable to complete the request. Try again. If it keeps failing, contact your administrator.",
    setup_title: "Set up a passkey",
    setup_subtitle: "This one-time link adds a passkey to your account.",
    setup_label: "Passkey name",
    setup_label_hint: "Optional. A name such as “Work laptop” helps you recognize it later.",
    setup_create: "Create passkey",
    setup_done_title: "Passkey created",
    setup_done_body: "Sign in with this passkey from your app or kubectl. You can close this page.",
    setup_invalid: "This setup link is invalid, already used, or expired. Ask your cluster administrator for a new link.",
    setup_missing_token: "This setup link is incomplete. Open the full link, including the part after “#”.",
    error_title: "Unable to continue sign-in",
    error_body: "Go back to the app and start again. If this keeps happening, contact your administrator.",
    error_detail_label: "Details",
    continue_with: "Continue with %provider%",
    upstream_or: "or",
    upstream_linked_title: "Account linked",
    upstream_linked_body: "You can now sign in with this account. You can close this page.",
    upstream_not_linked: "This account isn’t linked to any user. Ask your administrator for an invitation link, open it, and link the account there.",
    upstream_failed: "Unable to finish signing in with the provider. Start again from your app or invitation link.",
    upstream_already_linked: "This account is already linked to a user. Sign in with it, or link a different account.",
};

/// Traditional Chinese (Taiwan).
pub const ZH_TW: Messages = Messages {
    language_name: "繁體中文",
    nav_language: "語言",
    login_title: "登入",
    login_subtitle: "使用這台裝置或手機上的 Passkey。",
    login_continue_to: "繼續前往",
    login_username: "使用者名稱",
    login_username_hint: "選填。瀏覽器可以在這裡列出可用的 Passkey。",
    login_passkey: "使用 Passkey 登入",
    passkey_failed: "無法驗證這個 Passkey。再試一次，或改用其他 Passkey。",
    passkey_cancelled: "Passkey 對話窗在完成前就關閉了。再試一次。",
    passkey_unsupported_title: "這個瀏覽器無法使用 Passkey",
    passkey_unsupported_body: "改用最新版的 Safari、Chrome、Edge 或 Firefox 開啟這個頁面，或在手機上開啟。",
    noscript_title: "JavaScript 已關閉",
    noscript_body: "Passkey 需要 JavaScript。請為這個網站開啟 JavaScript，再重新載入頁面。",
    no_request_title: "目前沒有登入要求",
    no_request_body: "請從要使用的應用程式或指令（例如 kubectl）開始。需要登入時，它會開啟這個頁面。",
    elevated_title: "要求提升權限",
    elevated_body: "%app% 要求提升權限，有效 %minutes%\u{a0}分鐘。如果這次登入不是你自己從 %app% 發起的，請不要繼續。",
    working: "正在等待 Passkey…",
    generic_error: "無法完成操作。再試一次；如果仍然失敗，請聯絡管理員。",
    setup_title: "設定 Passkey",
    setup_subtitle: "這個連結只能用一次，可為帳號新增 Passkey。",
    setup_label: "Passkey 名稱",
    setup_label_hint: "選填。例如「公司筆電」，方便日後辨認。",
    setup_create: "建立 Passkey",
    setup_done_title: "已建立 Passkey",
    setup_done_body: "之後在應用程式或 kubectl 中用這個 Passkey 登入。現在可以關閉這個頁面。",
    setup_invalid: "這個設定連結無效、已使用過或已過期。請向叢集管理員索取新的連結。",
    setup_missing_token: "設定連結不完整。開啟完整的連結，含「#」之後的部分。",
    error_title: "無法繼續登入",
    error_body: "回到應用程式重新開始。如果一直發生，請聯絡管理員。",
    error_detail_label: "詳細資訊",
    continue_with: "使用 %provider% 繼續",
    upstream_or: "或",
    upstream_linked_title: "已連結帳號",
    upstream_linked_body: "之後可以用這個帳號登入。現在可以關閉這個頁面。",
    upstream_not_linked: "這個帳號沒有連結到任何使用者。請向管理員索取邀請連結，開啟後在那裡連結這個帳號。",
    upstream_failed: "無法透過外部帳號完成登入。從應用程式或邀請連結重新開始。",
    upstream_already_linked: "這個帳號已經連結到某位使用者。直接用它登入，或改連結其他帳號。",
};

/// Simplified Chinese.
pub const ZH_CN: Messages = Messages {
    language_name: "简体中文",
    nav_language: "语言",
    login_title: "登录",
    login_subtitle: "使用此设备或手机上的通行密钥。",
    login_continue_to: "继续前往",
    login_username: "用户名",
    login_username_hint: "选填。浏览器可以在这里列出可用的通行密钥。",
    login_passkey: "使用通行密钥登录",
    passkey_failed: "无法验证通行密钥。请重试或改用其他通行密钥。",
    passkey_cancelled: "通行密钥对话框在完成前就关闭了。再试一次。",
    passkey_unsupported_title: "此浏览器无法使用通行密钥",
    passkey_unsupported_body: "改用最新版的 Safari、Chrome、Edge 或 Firefox 打开此页面，或在手机上打开。",
    noscript_title: "JavaScript 已关闭",
    noscript_body: "通行密钥需要 JavaScript。请为此网站开启 JavaScript，然后重新加载页面。",
    no_request_title: "当前没有登录请求",
    no_request_body: "请从要使用的应用或命令（例如 kubectl）开始。需要登录时，它会打开此页面。",
    elevated_title: "请求提升权限",
    elevated_body: "%app% 请求提升权限，有效期 %minutes%\u{a0}分钟。如果这次登录不是你自己从 %app% 发起的，不要继续。",
    working: "正在等待通行密钥…",
    generic_error: "无法完成操作。再试一次；如果仍然失败，请联系管理员。",
    setup_title: "设置通行密钥",
    setup_subtitle: "此一次性链接会为你的账号添加通行密钥。",
    setup_label: "通行密钥名称",
    setup_label_hint: "选填。例如“工作笔记本”，方便以后辨认。",
    setup_create: "创建通行密钥",
    setup_done_title: "已创建通行密钥",
    setup_done_body: "之后在应用或 kubectl 中用此通行密钥登录。现在可以关闭此页面。",
    setup_invalid: "设置链接无效、已使用或已过期。请向集群管理员索取新链接。",
    setup_missing_token: "设置链接不完整。打开完整链接，含“#”之后的部分。",
    error_title: "无法继续登录",
    error_body: "返回应用重新开始。如果一直出现，请联系管理员。",
    error_detail_label: "详细信息",
    continue_with: "使用 %provider% 继续",
    upstream_or: "或",
    upstream_linked_title: "已关联账号",
    upstream_linked_body: "之后可以用这个账号登录。现在可以关闭此页面。",
    upstream_not_linked: "这个账号没有关联到任何用户。请向管理员索取邀请链接，打开后在那里关联此账号。",
    upstream_failed: "无法通过外部账号完成登录。从应用或邀请链接重新开始。",
    upstream_already_linked: "这个账号已关联到某个用户。直接用它登录，或改为关联其他账号。",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation() {
        assert_eq!(Lang::negotiate(None, None), Lang::En);
        assert_eq!(Lang::negotiate(Some("zh-TW"), Some("en")), Lang::ZhTw);
        assert_eq!(
            Lang::negotiate(Some("fr"), Some("zh-CN")),
            Lang::ZhCn,
            "unsupported explicit falls through"
        );
        assert_eq!(
            Lang::negotiate(None, Some("zh-Hant-HK,en;q=0.5")),
            Lang::ZhTw
        );
        assert_eq!(Lang::negotiate(None, Some("zh-Hans")), Lang::ZhCn);
        assert_eq!(Lang::negotiate(None, Some("zh")), Lang::ZhCn);
        assert_eq!(
            Lang::negotiate(None, Some("fr, en;q=0.4, zh-TW;q=0.8")),
            Lang::ZhTw,
            "by quality"
        );
        assert_eq!(
            Lang::negotiate(None, Some("zh-TW;q=0, en")),
            Lang::En,
            "q=0 excluded"
        );
        assert_eq!(Lang::negotiate(None, Some("de")), Lang::En);
    }

    #[test]
    fn qvalues() {
        assert_eq!(qvalue("1"), Some(1000));
        assert_eq!(qvalue("1.000"), Some(1000));
        assert_eq!(qvalue("0.5"), Some(500));
        assert_eq!(qvalue("0.125"), Some(125));
        assert_eq!(qvalue("0"), Some(0));
        for bad in ["", "1.5", "2", "0.1234", "-0", "0.-1", "abc", ".5"] {
            assert_eq!(qvalue(bad), None, "{bad}");
        }
    }

    #[test]
    fn negotiate_never_panics_on_garbage() {
        for input in ["", ",,,", ";q=", "zh;q=abc", "*;q=0.5", &"x,".repeat(1000)] {
            assert!(Lang::ALL.contains(&Lang::negotiate(Some(input), Some(input))));
        }
    }

    #[test]
    fn every_language_has_text() {
        for lang in Lang::ALL {
            let m = lang.messages();
            assert!(
                !m.login_title.is_empty() && !m.setup_done_body.is_empty(),
                "{lang:?}"
            );
            assert!(
                m.elevated_body.contains("%app%") && m.elevated_body.contains("%minutes%"),
                "{lang:?}"
            );
            assert!(m.continue_with.contains("%provider%"), "{lang:?}");
        }
    }
}
