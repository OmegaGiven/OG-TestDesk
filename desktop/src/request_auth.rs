//! Auth type selector for the Requests builder: None / Bearer / Basic /
//! API Key / OAuth 2.0, plus the header/query contributions each type
//! produces at send time.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use iced::widget::{button, column, pick_list, row, text, text_input};
use iced::{Element, Length};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    None,
    Bearer,
    Basic,
    ApiKey,
    OAuth2,
}

impl AuthType {
    pub const ALL: [AuthType; 5] = [
        AuthType::None,
        AuthType::Bearer,
        AuthType::Basic,
        AuthType::ApiKey,
        AuthType::OAuth2,
    ];
}

impl std::fmt::Display for AuthType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            AuthType::None => "None",
            AuthType::Bearer => "Bearer",
            AuthType::Basic => "Basic",
            AuthType::ApiKey => "API Key",
            AuthType::OAuth2 => "OAuth 2.0",
        };
        f.write_str(label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyLocation {
    Header,
    Query,
}

impl std::fmt::Display for ApiKeyLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ApiKeyLocation::Header => "Header",
            ApiKeyLocation::Query => "Query param",
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuthState {
    pub auth_type_index: usize,
    pub bearer_token: String,
    pub basic_user: String,
    pub basic_pass: String,
    pub api_key_name: String,
    pub api_key_value: String,
    pub api_key_in_query: bool,
    pub oauth_token_url: String,
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub oauth_scope: String,
    pub oauth_fetched_token: Option<String>,
    pub oauth_fetching: bool,
    pub oauth_error: Option<String>,
}

impl AuthState {
    pub fn auth_type(&self) -> AuthType {
        AuthType::ALL[self.auth_type_index.min(AuthType::ALL.len() - 1)]
    }

    /// Headers this auth method contributes to the outgoing request.
    pub fn effective_headers(&self) -> Vec<(String, String)> {
        match self.auth_type() {
            AuthType::None => vec![],
            AuthType::Bearer => {
                if self.bearer_token.trim().is_empty() {
                    vec![]
                } else {
                    vec![("Authorization".to_string(), format!("Bearer {}", self.bearer_token))]
                }
            }
            AuthType::Basic => {
                let raw = format!("{}:{}", self.basic_user, self.basic_pass);
                vec![(
                    "Authorization".to_string(),
                    format!("Basic {}", STANDARD.encode(raw)),
                )]
            }
            AuthType::ApiKey => {
                if self.api_key_in_query || self.api_key_name.trim().is_empty() {
                    vec![]
                } else {
                    vec![(self.api_key_name.clone(), self.api_key_value.clone())]
                }
            }
            AuthType::OAuth2 => match &self.oauth_fetched_token {
                Some(token) if !token.is_empty() => {
                    vec![("Authorization".to_string(), format!("Bearer {token}"))]
                }
                _ => vec![],
            },
        }
    }

    /// Query params this auth method contributes (only API Key-in-query).
    pub fn effective_query_params(&self) -> Vec<(String, String)> {
        match self.auth_type() {
            AuthType::ApiKey if self.api_key_in_query && !self.api_key_name.trim().is_empty() => {
                vec![(self.api_key_name.clone(), self.api_key_value.clone())]
            }
            _ => vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub enum AuthMessage {
    TypeSelected(AuthType),
    BearerTokenChanged(String),
    BasicUserChanged(String),
    BasicPassChanged(String),
    ApiKeyNameChanged(String),
    ApiKeyValueChanged(String),
    ApiKeyLocationSelected(ApiKeyLocation),
    OAuthTokenUrlChanged(String),
    OAuthClientIdChanged(String),
    OAuthClientSecretChanged(String),
    OAuthScopeChanged(String),
    FetchTokenPressed,
}

pub fn update(state: &mut AuthState, message: AuthMessage) {
    match message {
        AuthMessage::TypeSelected(t) => {
            state.auth_type_index = AuthType::ALL.iter().position(|x| *x == t).unwrap_or(0);
        }
        AuthMessage::BearerTokenChanged(v) => state.bearer_token = v,
        AuthMessage::BasicUserChanged(v) => state.basic_user = v,
        AuthMessage::BasicPassChanged(v) => state.basic_pass = v,
        AuthMessage::ApiKeyNameChanged(v) => state.api_key_name = v,
        AuthMessage::ApiKeyValueChanged(v) => state.api_key_value = v,
        AuthMessage::ApiKeyLocationSelected(loc) => {
            state.api_key_in_query = matches!(loc, ApiKeyLocation::Query);
        }
        AuthMessage::OAuthTokenUrlChanged(v) => state.oauth_token_url = v,
        AuthMessage::OAuthClientIdChanged(v) => state.oauth_client_id = v,
        AuthMessage::OAuthClientSecretChanged(v) => state.oauth_client_secret = v,
        AuthMessage::OAuthScopeChanged(v) => state.oauth_scope = v,
        AuthMessage::FetchTokenPressed => {
            // Handled by the parent tab, which owns the async proxy call.
        }
    }
}

pub fn view(state: &AuthState) -> Element<'_, AuthMessage> {
    let type_picker = pick_list(&AuthType::ALL[..], Some(state.auth_type()), AuthMessage::TypeSelected);

    let fields: Element<'_, AuthMessage> = match state.auth_type() {
        AuthType::None => text("No authentication").into(),
        AuthType::Bearer => text_input("Token", &state.bearer_token)
            .on_input(AuthMessage::BearerTokenChanged)
            .into(),
        AuthType::Basic => column![
            text_input("Username", &state.basic_user).on_input(AuthMessage::BasicUserChanged),
            text_input("Password", &state.basic_pass)
                .on_input(AuthMessage::BasicPassChanged)
                .secure(true),
        ]
        .spacing(6)
        .into(),
        AuthType::ApiKey => column![
            text_input("Key name", &state.api_key_name).on_input(AuthMessage::ApiKeyNameChanged),
            text_input("Key value", &state.api_key_value).on_input(AuthMessage::ApiKeyValueChanged),
            pick_list(
                &[ApiKeyLocation::Header, ApiKeyLocation::Query][..],
                Some(if state.api_key_in_query {
                    ApiKeyLocation::Query
                } else {
                    ApiKeyLocation::Header
                }),
                AuthMessage::ApiKeyLocationSelected,
            ),
        ]
        .spacing(6)
        .into(),
        AuthType::OAuth2 => {
            let fetch_label = if state.oauth_fetching { "Fetching..." } else { "Fetch token" };
            let mut col = column![
                text_input("Token URL", &state.oauth_token_url).on_input(AuthMessage::OAuthTokenUrlChanged),
                text_input("Client ID", &state.oauth_client_id).on_input(AuthMessage::OAuthClientIdChanged),
                text_input("Client Secret", &state.oauth_client_secret)
                    .on_input(AuthMessage::OAuthClientSecretChanged)
                    .secure(true),
                text_input("Scope", &state.oauth_scope).on_input(AuthMessage::OAuthScopeChanged),
                row![
                    button(text(fetch_label)).on_press(AuthMessage::FetchTokenPressed),
                    text(match &state.oauth_fetched_token {
                        Some(_) => "Token acquired".to_string(),
                        None => "No token yet".to_string(),
                    }),
                ]
                .spacing(8),
            ]
            .spacing(6);
            if let Some(err) = &state.oauth_error {
                col = col.push(text(err.clone()).color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)));
            }
            col.into()
        }
    };

    column![row![text("Type:"), type_picker].spacing(8).width(Length::Fill), fields]
        .spacing(10)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_produces_authorization_header() {
        let mut state = AuthState::default();
        state.auth_type_index = 1;
        state.bearer_token = "abc123".to_string();
        assert_eq!(
            state.effective_headers(),
            vec![("Authorization".to_string(), "Bearer abc123".to_string())]
        );
    }

    #[test]
    fn bearer_with_empty_token_produces_no_header() {
        let mut state = AuthState::default();
        state.auth_type_index = 1;
        assert!(state.effective_headers().is_empty());
    }

    #[test]
    fn basic_encodes_user_pass_as_base64() {
        let mut state = AuthState::default();
        state.auth_type_index = 2;
        state.basic_user = "alice".to_string();
        state.basic_pass = "secret".to_string();
        let headers = state.effective_headers();
        assert_eq!(headers[0].0, "Authorization");
        assert!(headers[0].1.starts_with("Basic "));
        let encoded = headers[0].1.strip_prefix("Basic ").unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "alice:secret");
    }

    #[test]
    fn api_key_in_header_vs_query() {
        let mut state = AuthState::default();
        state.auth_type_index = 3;
        state.api_key_name = "X-Api-Key".to_string();
        state.api_key_value = "k1".to_string();
        assert_eq!(
            state.effective_headers(),
            vec![("X-Api-Key".to_string(), "k1".to_string())]
        );
        assert!(state.effective_query_params().is_empty());

        state.api_key_in_query = true;
        assert!(state.effective_headers().is_empty());
        assert_eq!(
            state.effective_query_params(),
            vec![("X-Api-Key".to_string(), "k1".to_string())]
        );
    }

    #[test]
    fn oauth2_uses_fetched_token_when_present() {
        let mut state = AuthState::default();
        state.auth_type_index = 4;
        assert!(state.effective_headers().is_empty());
        state.oauth_fetched_token = Some("tok-xyz".to_string());
        assert_eq!(
            state.effective_headers(),
            vec![("Authorization".to_string(), "Bearer tok-xyz".to_string())]
        );
    }

    #[test]
    fn none_produces_nothing() {
        let state = AuthState::default();
        assert!(state.effective_headers().is_empty());
        assert!(state.effective_query_params().is_empty());
    }
}
