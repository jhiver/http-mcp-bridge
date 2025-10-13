use clap::{Parser, Subcommand};
use saramcp::{
    db,
    repositories::user_repository::SqliteUserRepository,
    services::user_service::{CreateUserRequest, UpdatePasswordRequest, UserService},
};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "saramcp-cli")]
#[command(about = "CLI tool for managing SaraMCP users", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// User management commands
    User {
        #[command(subcommand)]
        command: UserCommands,
    },
}

#[derive(Subcommand)]
enum UserCommands {
    /// Create a new user
    Create {
        /// Email address
        #[arg(short, long)]
        email: String,

        /// Password (will prompt if not provided)
        #[arg(short, long)]
        password: Option<String>,

        /// Mark email as verified
        #[arg(long)]
        verified: bool,
    },

    /// List all users
    List {
        /// Maximum number of users to display
        #[arg(short, long, default_value_t = 100)]
        limit: i64,

        /// Offset for pagination
        #[arg(short = 'o', long, default_value_t = 0)]
        offset: i64,
    },

    /// Delete a user
    Delete {
        /// Email address of the user to delete
        #[arg(short, long)]
        email: String,
    },

    /// Verify a user's email
    Verify {
        /// Email address of the user to verify
        #[arg(short, long)]
        email: String,
    },

    /// Set a new password for a user
    SetPassword {
        /// Email address of the user
        #[arg(short, long)]
        email: String,

        /// New password (will prompt if not provided)
        #[arg(short, long)]
        password: Option<String>,
    },
}

async fn get_password(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::{self, Write};
    print!("{}: ", prompt);
    io::stdout().flush()?;

    Ok(rpassword::read_password()?)
}

async fn confirm_password(prompt: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let password = get_password(prompt).await?;
    let confirm = get_password("Confirm password").await?;
    Ok((password, confirm))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Connect to database
    let pool = db::create_pool().await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    // Initialize services
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository));

    // Parse CLI arguments
    let cli = Cli::parse();

    match cli.command {
        Commands::User { command } => match command {
            UserCommands::Create {
                email,
                password,
                verified,
            } => {
                let (password, password_confirm) = if let Some(pw) = password {
                    (pw.clone(), pw)
                } else {
                    confirm_password("Password").await?
                };

                if password != password_confirm {
                    eprintln!("❌ Passwords do not match");
                    std::process::exit(1);
                }

                let request = CreateUserRequest {
                    email: email.clone(),
                    password,
                    password_confirm: Some(password_confirm),
                    email_verified: verified,
                };

                match user_service.create_user(request).await {
                    Ok(user) => {
                        println!("✅ User created successfully!");
                        println!("  ID: {}", user.id);
                        println!("  Email: {}", user.email);
                        println!("  Verified: {}", user.email_verified);
                    }
                    Err(err) => {
                        eprintln!("❌ Failed to create user: {}", err);
                        std::process::exit(1);
                    }
                }
            }

            UserCommands::List { limit, offset } => {
                match user_service.list_users(Some(limit), Some(offset)).await {
                    Ok(users) => {
                        if users.is_empty() {
                            println!("No users found.");
                        } else {
                            println!(
                                "{:<5} {:<40} {:<10} {:<20}",
                                "ID", "Email", "Verified", "Created"
                            );
                            println!("{}", "-".repeat(75));
                            for user in users {
                                println!(
                                    "{:<5} {:<40} {:<10} {:<20}",
                                    user.id,
                                    user.email,
                                    if user.email_verified { "Yes" } else { "No" },
                                    user.created_at.as_deref().unwrap_or("N/A")
                                );
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("❌ Failed to list users: {}", err);
                        std::process::exit(1);
                    }
                }
            }

            UserCommands::Delete { email } => match user_service.find_user_by_email(&email).await {
                Ok(Some(user)) => match user_service.delete_user(user.id).await {
                    Ok(()) => {
                        println!("✅ User '{}' deleted successfully!", email);
                    }
                    Err(err) => {
                        eprintln!("❌ Failed to delete user: {}", err);
                        std::process::exit(1);
                    }
                },
                Ok(None) => {
                    eprintln!("❌ User '{}' not found", email);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("❌ Failed to find user: {}", err);
                    std::process::exit(1);
                }
            },

            UserCommands::Verify { email } => match user_service.find_user_by_email(&email).await {
                Ok(Some(user)) => {
                    if user.email_verified {
                        println!("ℹ️  User '{}' is already verified", email);
                    } else {
                        match user_service.verify_user_email(user.id).await {
                            Ok(()) => {
                                println!("✅ User '{}' email verified successfully!", email);
                            }
                            Err(err) => {
                                eprintln!("❌ Failed to verify user: {}", err);
                                std::process::exit(1);
                            }
                        }
                    }
                }
                Ok(None) => {
                    eprintln!("❌ User '{}' not found", email);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("❌ Failed to find user: {}", err);
                    std::process::exit(1);
                }
            },

            UserCommands::SetPassword { email, password } => {
                match user_service.find_user_by_email(&email).await {
                    Ok(Some(user)) => {
                        let (new_password, password_confirm) = if let Some(pw) = password {
                            (pw.clone(), pw)
                        } else {
                            confirm_password("New password").await?
                        };

                        let request = UpdatePasswordRequest {
                            user_id: user.id,
                            new_password,
                            new_password_confirm: Some(password_confirm),
                        };

                        match user_service.update_password(request).await {
                            Ok(()) => {
                                println!("✅ Password updated successfully for '{}'!", email);
                            }
                            Err(err) => {
                                eprintln!("❌ Failed to update password: {}", err);
                                std::process::exit(1);
                            }
                        }
                    }
                    Ok(None) => {
                        eprintln!("❌ User '{}' not found", email);
                        std::process::exit(1);
                    }
                    Err(err) => {
                        eprintln!("❌ Failed to find user: {}", err);
                        std::process::exit(1);
                    }
                }
            }
        },
    }

    Ok(())
}
