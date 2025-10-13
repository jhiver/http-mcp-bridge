use async_trait::async_trait;
use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use std::env;

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("Failed to build email message: {0}")]
    MessageBuild(String),
    #[error("Failed to send email: {0}")]
    SendFailed(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

#[async_trait]
pub trait EmailService: Send + Sync {
    async fn send_verification_email(&self, to_email: &str, token: &str) -> Result<(), EmailError>;
    async fn send_magic_login_email(&self, to_email: &str, token: &str) -> Result<(), EmailError>;
    async fn send_contact_form(
        &self,
        from_email: &str,
        from_name: Option<&str>,
        message: &str,
    ) -> Result<(), EmailError>;
}

pub struct MockEmailService {
    base_url: String,
}

impl MockEmailService {
    pub fn new() -> Self {
        let base_url = env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        Self { base_url }
    }
}

impl Default for MockEmailService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmailService for MockEmailService {
    async fn send_verification_email(&self, to_email: &str, token: &str) -> Result<(), EmailError> {
        let verification_url = format!("{}/auth/verify/{}", self.base_url, token);
        tracing::info!("📧 [MOCK EMAIL] Verification email to: {}", to_email);
        tracing::info!("   Subject: Verify your SaraMCP account");
        tracing::info!("   Verification link: {}", verification_url);
        tracing::info!("   ---");
        Ok(())
    }

    async fn send_magic_login_email(&self, to_email: &str, token: &str) -> Result<(), EmailError> {
        let magic_link_url = format!("{}/auth/magic/{}", self.base_url, token);
        tracing::info!("📧 [MOCK EMAIL] Magic login link to: {}", to_email);
        tracing::info!("   Subject: Your SaraMCP login link");
        tracing::info!("   Magic link: {}", magic_link_url);
        tracing::info!("   ---");
        Ok(())
    }

    async fn send_contact_form(
        &self,
        from_email: &str,
        from_name: Option<&str>,
        message: &str,
    ) -> Result<(), EmailError> {
        tracing::info!("📧 [MOCK EMAIL] Contact form submission");
        tracing::info!("   From: {}", from_email);
        if let Some(name) = from_name {
            tracing::info!("   Name: {}", name);
        }
        tracing::info!("   Message: {}", message);
        tracing::info!("   ---");
        Ok(())
    }
}

pub struct SmtpEmailService {
    mailer: AsyncSmtpTransport<Tokio1Executor>,
    from_email: String,
    from_name: String,
    base_url: String,
}

impl SmtpEmailService {
    pub fn new() -> Result<Self, EmailError> {
        let smtp_host = env::var("SMTP_HOST")
            .map_err(|_| EmailError::ConfigError("SMTP_HOST not set".to_string()))?;
        let smtp_port = env::var("SMTP_PORT")
            .unwrap_or_else(|_| "587".to_string())
            .parse::<u16>()
            .map_err(|_| EmailError::ConfigError("Invalid SMTP_PORT".to_string()))?;
        let smtp_username = env::var("SMTP_USERNAME")
            .map_err(|_| EmailError::ConfigError("SMTP_USERNAME not set".to_string()))?;
        let smtp_password = env::var("SMTP_PASSWORD")
            .map_err(|_| EmailError::ConfigError("SMTP_PASSWORD not set".to_string()))?;
        let from_email = env::var("SMTP_FROM_EMAIL")
            .map_err(|_| EmailError::ConfigError("SMTP_FROM_EMAIL not set".to_string()))?;
        let from_name = env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "SaraMCP".to_string());
        let base_url = env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());

        let encryption = env::var("SMTP_ENCRYPTION").unwrap_or_else(|_| "starttls".to_string());

        let credentials = Credentials::new(smtp_username, smtp_password);

        let mailer = match encryption.to_lowercase().as_str() {
            "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp_host)
                .map_err(|e| EmailError::ConfigError(format!("SMTP relay error: {}", e)))?
                .port(smtp_port)
                .credentials(credentials)
                .build(),
            "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp_host)
                .map_err(|e| EmailError::ConfigError(format!("SMTP starttls error: {}", e)))?
                .port(smtp_port)
                .credentials(credentials)
                .build(),
            "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp_host)
                .port(smtp_port)
                .credentials(credentials)
                .build(),
            _ => {
                return Err(EmailError::ConfigError(format!(
                    "Invalid SMTP_ENCRYPTION value: {}. Use 'tls', 'starttls', or 'none'",
                    encryption
                )))
            }
        };

        Ok(Self {
            mailer,
            from_email,
            from_name,
            base_url,
        })
    }
}

#[async_trait]
impl EmailService for SmtpEmailService {
    async fn send_verification_email(&self, to_email: &str, token: &str) -> Result<(), EmailError> {
        let verification_url = format!("{}/auth/verify/{}", self.base_url, token);

        let html_body = format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
</head>
<body style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px;">
    <h1 style="color: #333;">Welcome to SaraMCP!</h1>
    <p>Thank you for signing up. Please verify your email address by clicking the button below:</p>
    <p style="text-align: center; margin: 30px 0;">
        <a href="{}" style="background-color: #4CAF50; color: white; padding: 12px 24px; text-decoration: none; border-radius: 4px; display: inline-block;">Verify Email Address</a>
    </p>
    <p style="color: #666; font-size: 14px;">Or copy and paste this link into your browser:</p>
    <p style="color: #666; font-size: 14px; word-break: break-all;">{}</p>
    <p style="color: #999; font-size: 12px; margin-top: 40px;">This link will expire in 24 hours.</p>
</body>
</html>
"#,
            verification_url, verification_url
        );

        let email = Message::builder()
            .from(
                format!("{} <{}>", self.from_name, self.from_email)
                    .parse()
                    .map_err(|e| {
                        EmailError::MessageBuild(format!("Invalid from address: {}", e))
                    })?,
            )
            .to(to_email
                .parse()
                .map_err(|e| EmailError::MessageBuild(format!("Invalid to address: {}", e)))?)
            .subject("Verify your SaraMCP account")
            .header(ContentType::TEXT_HTML)
            .body(html_body)
            .map_err(|e| EmailError::MessageBuild(e.to_string()))?;

        self.mailer
            .send(email)
            .await
            .map_err(|e| EmailError::SendFailed(e.to_string()))?;

        Ok(())
    }

    async fn send_magic_login_email(&self, to_email: &str, token: &str) -> Result<(), EmailError> {
        let magic_link_url = format!("{}/auth/magic/{}", self.base_url, token);

        let html_body = format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
</head>
<body style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px;">
    <h1 style="color: #333;">Your SaraMCP Login Link</h1>
    <p>Click the button below to log in to your account:</p>
    <p style="text-align: center; margin: 30px 0;">
        <a href="{}" style="background-color: #2196F3; color: white; padding: 12px 24px; text-decoration: none; border-radius: 4px; display: inline-block;">Log In to SaraMCP</a>
    </p>
    <p style="color: #666; font-size: 14px;">Or copy and paste this link into your browser:</p>
    <p style="color: #666; font-size: 14px; word-break: break-all;">{}</p>
    <p style="color: #999; font-size: 12px; margin-top: 40px;">This link will expire in 15 minutes. If you didn't request this login link, you can safely ignore this email.</p>
</body>
</html>
"#,
            magic_link_url, magic_link_url
        );

        let email = Message::builder()
            .from(
                format!("{} <{}>", self.from_name, self.from_email)
                    .parse()
                    .map_err(|e| {
                        EmailError::MessageBuild(format!("Invalid from address: {}", e))
                    })?,
            )
            .to(to_email
                .parse()
                .map_err(|e| EmailError::MessageBuild(format!("Invalid to address: {}", e)))?)
            .subject("Your SaraMCP login link")
            .header(ContentType::TEXT_HTML)
            .body(html_body)
            .map_err(|e| EmailError::MessageBuild(e.to_string()))?;

        self.mailer
            .send(email)
            .await
            .map_err(|e| EmailError::SendFailed(e.to_string()))?;

        Ok(())
    }

    async fn send_contact_form(
        &self,
        from_email: &str,
        from_name: Option<&str>,
        message: &str,
    ) -> Result<(), EmailError> {
        let name_display = from_name.map(|n| format!(" ({})", n)).unwrap_or_default();

        let html_body = format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
</head>
<body style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px;">
    <h1 style="color: #333;">Contact Form Submission</h1>
    <div style="background-color: #f5f5f5; padding: 15px; border-radius: 4px; margin: 20px 0;">
        <p style="margin: 5px 0;"><strong>From:</strong> {}{}</p>
        <p style="margin: 5px 0;"><strong>Email:</strong> {}</p>
    </div>
    <div style="background-color: #fff; padding: 15px; border: 1px solid #ddd; border-radius: 4px;">
        <p style="margin: 0 0 10px 0;"><strong>Message:</strong></p>
        <p style="margin: 0; white-space: pre-wrap;">{}</p>
    </div>
</body>
</html>
"#,
            from_email, name_display, from_email, message
        );

        let subject = format!("Contact Form Submission from {}", from_email);

        let email = Message::builder()
            .from(
                format!("{} <{}>", self.from_name, self.from_email)
                    .parse()
                    .map_err(|e| {
                        EmailError::MessageBuild(format!("Invalid from address: {}", e))
                    })?,
            )
            .reply_to(from_email.parse().map_err(|e| {
                EmailError::MessageBuild(format!("Invalid reply-to address: {}", e))
            })?)
            .to("jhiver+saramcp@gmail.com"
                .parse()
                .map_err(|e| EmailError::MessageBuild(format!("Invalid to address: {}", e)))?)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(html_body)
            .map_err(|e| EmailError::MessageBuild(e.to_string()))?;

        self.mailer
            .send(email)
            .await
            .map_err(|e| EmailError::SendFailed(e.to_string()))?;

        Ok(())
    }
}

pub fn create_email_service() -> Box<dyn EmailService> {
    if env::var("SMTP_HOST").is_ok() {
        match SmtpEmailService::new() {
            Ok(service) => {
                tracing::info!("Using SMTP email service");
                Box::new(service)
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to initialize SMTP email service: {}. Falling back to mock service",
                    e
                );
                Box::new(MockEmailService::new())
            }
        }
    } else {
        tracing::info!(
            "SMTP not configured. Using mock email service (emails will be logged to console)"
        );
        Box::new(MockEmailService::new())
    }
}
