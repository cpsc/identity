#![doc = include_str!("../README.md")]

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use genkan_core::clock::UnixTime;
use genkan_core::crypto::{hex, random};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use thiserror::Error;
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader,
};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// Time allowed for the whole conversation.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Longest reply line accepted (RFC 5321 §4.5.3.1.5 allows 512 octets).
const MAX_LINE: usize = 1024;

/// Why mail could not be sent.
#[derive(Debug, Error)]
pub enum MailError {
    /// The configuration is unusable.
    #[error("mail configuration: {0}")]
    Config(&'static str),
    /// An address is not acceptable.
    #[error("not an acceptable email address")]
    Address,
    /// Network or TLS failure.
    #[error("connection: {0}")]
    Io(#[from] std::io::Error),
    /// The server answered with an unexpected code.
    #[error("the SMTP server refused {stage}: {reply}")]
    Refused {
        /// What was being done.
        stage: &'static str,
        /// The server's reply.
        reply: String,
    },
    /// The server broke the protocol (or tried to inject commands).
    #[error("SMTP protocol violation: {0}")]
    Protocol(&'static str),
    /// The conversation took too long.
    #[error("the SMTP server timed out")]
    Timeout,
}

/// How to reach the submission server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    /// TLS from the first byte (`smtps://`, default port 465).
    Implicit,
    /// Plain connection upgraded with STARTTLS, which is required (`smtp://`, default port 587).
    StartTls,
}

/// A mail submission server and the sender identity.
#[derive(Clone)]
pub struct Mailer {
    host: String,
    port: u16,
    security: Security,
    username: Option<String>,
    password: Option<String>,
    from: String,
    tls: Arc<ClientConfig>,
}

impl fmt::Debug for Mailer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mailer")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

/// Whether `address` is a plain `local@domain` address: printable ASCII without spaces, angle
/// brackets, quotes or line breaks, and a dotted domain. Stricter than RFC 5321, on purpose.
#[must_use]
pub fn acceptable_address(address: &str) -> bool {
    let Some((local, domain)) = address.rsplit_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+/=?^_`{|}~.-".contains(&b));
    let domain_ok = domain.len() <= 253
        && domain.contains('.')
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    local_ok && domain_ok && address.len() <= 254
}

impl Mailer {
    /// Parses `smtps://[user@]host[:port]` or `smtp://[user@]host[:port]`. `roots` are the CAs
    /// that may vouch for the server.
    ///
    /// # Errors
    ///
    /// [`MailError::Config`] for another scheme, a missing host, or an unacceptable sender.
    pub fn new(
        url: &str,
        password: Option<String>,
        from: &str,
        roots: rustls::RootCertStore,
    ) -> Result<Self, MailError> {
        let (security, rest) = if let Some(rest) = url.strip_prefix("smtps://") {
            (Security::Implicit, rest)
        } else if let Some(rest) = url.strip_prefix("smtp://") {
            (Security::StartTls, rest)
        } else {
            return Err(MailError::Config(
                "the URL must start with smtps:// or smtp://",
            ));
        };
        let rest = rest.trim_end_matches('/');
        let (username, authority) = match rest.rsplit_once('@') {
            Some((user, authority)) => (Some(user.to_owned()), authority),
            None => (None, rest),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (
                host,
                port.parse()
                    .map_err(|_port| MailError::Config("invalid port"))?,
            ),
            None => (
                authority,
                match security {
                    Security::Implicit => 465,
                    Security::StartTls => 587,
                },
            ),
        };
        if host.is_empty() || host.contains(['/', '?', '#']) {
            return Err(MailError::Config("invalid host"));
        }
        if !acceptable_address(from) {
            return Err(MailError::Config("invalid sender address"));
        }
        let tls = ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_tls| MailError::Config("TLS setup failed"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Ok(Self {
            host: host.to_owned(),
            port,
            security,
            username,
            password,
            from: from.to_owned(),
            tls: Arc::new(tls),
        })
    }

    /// Sends one message.
    ///
    /// # Errors
    ///
    /// [`MailError`] if the recipient is unacceptable or the server refuses or fails.
    pub async fn send(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        now: UnixTime,
    ) -> Result<(), MailError> {
        if !acceptable_address(to) {
            return Err(MailError::Address);
        }
        let message = self.message(to, subject, body, now)?;
        tokio::time::timeout(TIMEOUT, self.deliver(to, &message))
            .await
            .map_err(|_elapsed| MailError::Timeout)?
    }

    fn message(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        now: UnixTime,
    ) -> Result<String, MailError> {
        let domain = self.from.rsplit_once('@').map_or("localhost", |(_, d)| d);
        let id = hex(&random::<16>().map_err(|_crypto| MailError::Config("randomness failed"))?);
        let encoded = STANDARD.encode(body.as_bytes());
        let mut lines = vec![
            format!("From: <{}>", self.from),
            format!("To: <{to}>"),
            format!(
                "Subject: =?UTF-8?B?{}?=",
                STANDARD.encode(subject.as_bytes())
            ),
            format!("Date: {}", rfc5322_date(now)),
            format!("Message-ID: <{id}@{domain}>"),
            "MIME-Version: 1.0".to_owned(),
            "Content-Type: text/plain; charset=utf-8".to_owned(),
            "Content-Transfer-Encoding: base64".to_owned(),
            "Auto-Submitted: auto-generated".to_owned(),
            String::new(),
        ];
        // RFC 2045 §6.8: encoded lines of at most 76 characters. Base64 has no '.', so no line
        // needs dot-stuffing.
        lines.extend(
            encoded
                .as_bytes()
                .chunks(76)
                .map(|chunk| String::from_utf8_lossy(chunk).into_owned()),
        );
        Ok(lines.join("\r\n"))
    }

    async fn deliver(&self, to: &str, message: &str) -> Result<(), MailError> {
        let tcp = TcpStream::connect((self.host.as_str(), self.port)).await?;
        let name = ServerName::try_from(self.host.clone())
            .map_err(|_name| MailError::Config("invalid host"))?;
        let connector = TlsConnector::from(Arc::clone(&self.tls));
        match self.security {
            Security::Implicit => {
                let tls = connector.connect(name, tcp).await?;
                let mut session = BufReader::new(tls);
                expect(&mut session, "the greeting", 220).await?;
                self.ehlo(&mut session).await?;
                self.transact(&mut session, to, message).await
            }
            Security::StartTls => {
                let mut plain = BufReader::new(tcp);
                expect(&mut plain, "the greeting", 220).await?;
                let capabilities = self.ehlo(&mut plain).await?;
                if !capabilities
                    .iter()
                    .any(|c| c.eq_ignore_ascii_case("STARTTLS"))
                {
                    return Err(MailError::Protocol("the server does not offer STARTTLS"));
                }
                command(&mut plain, "STARTTLS").await?;
                expect(&mut plain, "STARTTLS", 220).await?;
                // Anything already buffered was sent before TLS and could be an injected reply.
                if !plain.buffer().is_empty() {
                    return Err(MailError::Protocol("data before the TLS handshake"));
                }
                let tls = connector.connect(name, plain.into_inner()).await?;
                let mut session = BufReader::new(tls);
                // RFC 3207 §4.2: forget everything learnt before TLS; ask again.
                self.ehlo(&mut session).await?;
                self.transact(&mut session, to, message).await
            }
        }
    }

    async fn ehlo<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        session: &mut BufReader<S>,
    ) -> Result<Vec<String>, MailError> {
        let domain = self.from.rsplit_once('@').map_or("localhost", |(_, d)| d);
        command(session, &format!("EHLO {domain}")).await?;
        let (code, lines) = reply(session).await?;
        if code != 250 {
            return Err(refused("EHLO", code, &lines));
        }
        Ok(lines
            .iter()
            .skip(1)
            .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
            .collect())
    }

    async fn transact<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        session: &mut BufReader<S>,
        to: &str,
        message: &str,
    ) -> Result<(), MailError> {
        if let (Some(user), Some(password)) = (&self.username, &self.password) {
            let plain = STANDARD.encode(format!("\0{user}\0{password}"));
            command(session, &format!("AUTH PLAIN {plain}")).await?;
            expect(session, "authentication", 235).await?;
        }
        command(session, &format!("MAIL FROM:<{}>", self.from)).await?;
        expect(session, "the sender", 250).await?;
        command(session, &format!("RCPT TO:<{to}>")).await?;
        let (code, lines) = reply(session).await?;
        if code != 250 && code != 251 {
            return Err(refused("the recipient", code, &lines));
        }
        command(session, "DATA").await?;
        expect(session, "DATA", 354).await?;
        session.get_mut().write_all(message.as_bytes()).await?;
        command(session, "\r\n.").await?;
        expect(session, "the message", 250).await?;
        // A failed QUIT does not undo a delivered message.
        drop(command(session, "QUIT").await);
        Ok(())
    }
}

fn refused(stage: &'static str, code: u16, lines: &[String]) -> MailError {
    MailError::Refused {
        stage,
        reply: format!("{code} {}", lines.join(" / ")),
    }
}

async fn command<S: AsyncRead + AsyncWrite + Unpin>(
    session: &mut BufReader<S>,
    line: &str,
) -> Result<(), MailError> {
    let stream = session.get_mut();
    stream.write_all(line.as_bytes()).await?;
    stream.write_all(b"\r\n").await?;
    stream.flush().await?;
    Ok(())
}

/// Reads one (possibly multi-line) reply: its code and the text of each line.
async fn reply<S: AsyncRead + AsyncWrite + Unpin>(
    session: &mut BufReader<S>,
) -> Result<(u16, Vec<String>), MailError> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        let read = (&mut *session)
            .take(u64::try_from(MAX_LINE).unwrap_or(u64::MAX))
            .read_line(&mut line)
            .await?;
        if read == 0 || !line.ends_with('\n') {
            return Err(MailError::Protocol("truncated reply"));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let (Some(code), Some(separator)) = (
            line.get(..3).and_then(|c| c.parse::<u16>().ok()),
            line.as_bytes().get(3).copied().or(Some(b' ')),
        ) else {
            return Err(MailError::Protocol("malformed reply"));
        };
        lines.push(line.get(4..).unwrap_or_default().to_owned());
        match separator {
            b'-' => {}
            b' ' => return Ok((code, lines)),
            _ => return Err(MailError::Protocol("malformed reply")),
        }
        if lines.len() > 64 {
            return Err(MailError::Protocol("reply too long"));
        }
    }
}

async fn expect<S: AsyncRead + AsyncWrite + Unpin>(
    session: &mut BufReader<S>,
    stage: &'static str,
    wanted: u16,
) -> Result<(), MailError> {
    let (code, lines) = reply(session).await?;
    if code == wanted {
        Ok(())
    } else {
        Err(refused(stage, code, &lines))
    }
}

/// RFC 5322 §3.3 date in UTC, for example `Mon, 12 Oct 2026 03:04:05 +0000`.
#[must_use]
pub fn rfc5322_date(time: UnixTime) -> String {
    let at = i64::try_from(time.as_secs())
        .ok()
        .and_then(|secs| time::OffsetDateTime::from_unix_timestamp(secs).ok())
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let weekday = match at.weekday() {
        time::Weekday::Monday => "Mon",
        time::Weekday::Tuesday => "Tue",
        time::Weekday::Wednesday => "Wed",
        time::Weekday::Thursday => "Thu",
        time::Weekday::Friday => "Fri",
        time::Weekday::Saturday => "Sat",
        time::Weekday::Sunday => "Sun",
    };
    let month = match at.month() {
        time::Month::January => "Jan",
        time::Month::February => "Feb",
        time::Month::March => "Mar",
        time::Month::April => "Apr",
        time::Month::May => "May",
        time::Month::June => "Jun",
        time::Month::July => "Jul",
        time::Month::August => "Aug",
        time::Month::September => "Sep",
        time::Month::October => "Oct",
        time::Month::November => "Nov",
        time::Month::December => "Dec",
    };
    format!(
        "{weekday}, {:02} {month} {} {:02}:{:02}:{:02} +0000",
        at.day(),
        at.year(),
        at.hour(),
        at.minute(),
        at.second()
    )
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use genkan_tls::certificate::{certified_key, issue};
    use genkan_tls::listener::{Resolver, server_config};
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject as _;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio_rustls::TlsAcceptor;

    use super::*;

    #[test]
    fn addresses() {
        for good in ["a@example.com", "first.last+tag@sub.example.org"] {
            assert!(acceptable_address(good), "{good}");
        }
        for bad in [
            "",
            "a",
            "a@b",
            "@example.com",
            "a b@example.com",
            "a@example.com\r\nRCPT TO:<x@y.z>",
            "<a@example.com>",
            "a@-example.com",
            "\"q\"@example.com",
        ] {
            assert!(!acceptable_address(bad), "{bad}");
        }
    }

    #[test]
    fn dates() {
        assert_eq!(
            rfc5322_date(UnixTime::from_secs(0)),
            "Thu, 01 Jan 1970 00:00:00 +0000"
        );
        assert_eq!(
            rfc5322_date(UnixTime::from_secs(1_791_801_845)),
            "Mon, 12 Oct 2026 10:44:05 +0000"
        );
    }

    #[test]
    fn urls() {
        let roots = rustls::RootCertStore::empty;
        let m = Mailer::new("smtps://u@mail.example.com", None, "g@example.com", roots()).unwrap();
        assert_eq!((m.port, m.security), (465, Security::Implicit));
        let m = Mailer::new(
            "smtp://mail.example.com:2525",
            None,
            "g@example.com",
            roots(),
        )
        .unwrap();
        assert_eq!(
            (m.port, m.security, m.username),
            (2525, Security::StartTls, None)
        );
        for bad in ["http://x", "smtp://", "smtps://h:port"] {
            assert!(
                Mailer::new(bad, None, "g@example.com", roots()).is_err(),
                "{bad}"
            );
        }
        assert!(Mailer::new("smtp://h.example", None, "bad", roots()).is_err());
    }

    /// A one-message SMTP server for tests: optional STARTTLS, records the DATA it receives.
    async fn server(
        starttls: bool,
        inject: bool,
    ) -> (u16, rustls::RootCertStore, oneshot::Receiver<String>) {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let now = UnixTime::from_secs(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        let issued = issue(ip, &["localhost".into()], now, Duration::from_hours(1)).unwrap();
        let resolver = Arc::new(Resolver::default());
        resolver.set(certified_key(issued.cert_pem.as_bytes(), issued.key_pem.as_bytes()).unwrap());
        let mut config = (*server_config(resolver).unwrap()).clone();
        config.alpn_protocols.clear();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(issued.ca_pem.as_bytes()) {
            roots.add(cert.unwrap()).unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            if starttls {
                tcp.write_all(b"220 test ESMTP\r\n").await.unwrap();
                let mut reader = BufReader::new(&mut tcp);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap(); // EHLO
                reader
                    .get_mut()
                    .write_all(b"250-test\r\n250 STARTTLS\r\n")
                    .await
                    .unwrap();
                line.clear();
                reader.read_line(&mut line).await.unwrap(); // STARTTLS
                let ready: &[u8] = if inject {
                    b"220 go ahead\r\n250 injected\r\n"
                } else {
                    b"220 go ahead\r\n"
                };
                reader.get_mut().write_all(ready).await.unwrap();
                if inject {
                    return;
                }
                let tls = acceptor.accept(tcp).await.unwrap();
                dialogue(tls, false, tx).await;
            } else {
                let tls = acceptor.accept(tcp).await.unwrap();
                dialogue(tls, true, tx).await;
            }
        });
        (port, roots, rx)
    }

    async fn dialogue<S: AsyncRead + AsyncWrite + Unpin>(
        stream: S,
        greet: bool,
        tx: oneshot::Sender<String>,
    ) {
        let mut session = BufReader::new(stream);
        if greet {
            session
                .get_mut()
                .write_all(b"220 test ESMTP\r\n")
                .await
                .unwrap();
        }
        let mut transcript = String::new();
        loop {
            let mut line = String::new();
            if session.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            transcript.push_str(&line);
            let answer: &[u8] = match line.split_whitespace().next().unwrap_or_default() {
                "EHLO" => b"250-test\r\n250 AUTH PLAIN\r\n",
                "AUTH" => b"235 ok\r\n",
                "MAIL" | "RCPT" => b"250 ok\r\n",
                "DATA" => {
                    session.get_mut().write_all(b"354 go\r\n").await.unwrap();
                    let mut data = Vec::new();
                    while !data.ends_with(b"\r\n.\r\n") {
                        let mut byte = [0_u8; 1];
                        session.read_exact(&mut byte).await.unwrap();
                        data.push(byte[0]);
                    }
                    transcript.push_str(&String::from_utf8_lossy(&data));
                    b"250 queued\r\n"
                }
                "QUIT" => {
                    drop(session.get_mut().write_all(b"221 bye\r\n").await);
                    break;
                }
                _ => b"500 what\r\n",
            };
            session.get_mut().write_all(answer).await.unwrap();
        }
        drop(tx.send(transcript));
    }

    #[tokio::test]
    async fn sends_over_implicit_tls_with_auth() {
        let (port, roots, transcript) = server(false, false).await;
        let mailer = Mailer::new(
            &format!("smtps://genkan@localhost:{port}"),
            Some("hunter2".into()),
            "genkan@example.com",
            roots,
        )
        .unwrap();
        mailer
            .send(
                "dana@example.com",
                "邀請 Invitation",
                "Hello 你好\nline",
                UnixTime::from_secs(0),
            )
            .await
            .unwrap();
        let transcript = transcript.await.unwrap();
        assert!(transcript.contains(&format!(
            "AUTH PLAIN {}",
            STANDARD.encode("\0genkan\0hunter2")
        )));
        assert!(transcript.contains("RCPT TO:<dana@example.com>"));
        assert!(transcript.contains("Subject: =?UTF-8?B?"));
        assert!(transcript.contains(&STANDARD.encode("Hello 你好\nline")));
        assert!(transcript.contains("\r\n.\r\nQUIT"));
    }

    #[tokio::test]
    async fn upgrades_with_starttls() {
        let (port, roots, transcript) = server(true, false).await;
        let mailer = Mailer::new(
            &format!("smtp://localhost:{port}"),
            None,
            "genkan@example.com",
            roots,
        )
        .unwrap();
        mailer
            .send("dana@example.com", "s", "b", UnixTime::from_secs(0))
            .await
            .unwrap();
        // EHLO was repeated inside TLS, and no credentials were configured.
        let transcript = transcript.await.unwrap();
        assert!(transcript.starts_with("EHLO example.com"));
        assert!(!transcript.contains("AUTH"));
    }

    #[tokio::test]
    async fn refuses_replies_injected_before_tls() {
        let (port, roots, _transcript) = server(true, true).await;
        let mailer = Mailer::new(
            &format!("smtp://localhost:{port}"),
            None,
            "genkan@example.com",
            roots,
        )
        .unwrap();
        let error = mailer
            .send("dana@example.com", "s", "b", UnixTime::from_secs(0))
            .await
            .unwrap_err();
        assert!(matches!(error, MailError::Protocol(_)), "{error}");
    }

    #[tokio::test]
    async fn refuses_untrusted_servers_and_bad_recipients() {
        let (port, _roots, _transcript) = server(false, false).await;
        let mailer = Mailer::new(
            &format!("smtps://localhost:{port}"),
            None,
            "genkan@example.com",
            rustls::RootCertStore::empty(),
        )
        .unwrap();
        assert!(matches!(
            mailer
                .send("dana@example.com", "s", "b", UnixTime::from_secs(0))
                .await,
            Err(MailError::Io(_))
        ));
        assert!(matches!(
            mailer
                .send("x@y\r\nDATA", "s", "b", UnixTime::from_secs(0))
                .await,
            Err(MailError::Address)
        ));
    }
}
