//! Rust🦀 client for Apollo.
//!
//! Power by Rust `async/await`.
//!
//! ## Features
//!
//! Not all features are default, you can read the `[features]` section of [Cargo.toml](https://github.com/jmjoy/apollo-client/blob/master/Cargo.toml) to know all the features.
//!
//! The `xml` and `yaml` features aren't enable by default, if you have such kind namespace, you should add
//! `features` in `Cargo.toml`, just like:
//!
//! ```toml
//! apollo-client = { version = "0.4.0", features = ["yaml", "xml"] }
//! ```
//!
//! Or simply enable all features:
//!
//! ```toml
//! apollo-client = { version = "0.4.0", features = ["full"] }
//! ```
//!
//! ## Usage
//!
//! You can find some examples in [the examples directory](https://github.com/jmjoy/apollo-client/tree/master/examples).
//!
use futures::future::{join_all, select, Either};
use futures_timer::Delay;
use http::StatusCode;
use indexmap::map::IndexMap;
use isahc::HttpClientBuilder;
use isahc::ResponseExt;
use quick_error::quick_error;
use serde::de::DeserializeOwned;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::time::Duration;
use std::{fmt, io};

use isahc::config::{DnsCache, VersionNegotiation};
#[cfg(feature = "regex")]
use regex::Regex;

#[cfg(test)]
mod tests;

/// Default request config url timeout.
const DEFAULT_CONFIG_TIMEOUT: Duration = Duration::from_secs(30);

/// Should be longer than server side's long polling timeout, which is now 60 seconds.
const DEFAULT_LISTEN_TIMEOUT: Duration = Duration::from_secs(90);

/// Apollo client crate side `Result`.
pub type ApolloClientResult<T> = Result<T, ApolloClientError>;

quick_error! {
    /// Apollo client crate side `Error`.
    #[derive(Debug)]
    pub enum ApolloClientError {
        Io(err: io::Error) {
            from()
            description("io error")
            display("I/O error: {}", err)
            cause(err)
        }
        Isahc(err: isahc::Error) {
            from()
            description("isahc error")
            display("Isahc error: {}", err)
            cause(err)
        }
        SerdeJson(err: serde_json::error::Error) {
            from()
            description("serde json error")
            display("Serde json error: {}", err)
            cause(err)
        }
        SerdeUrlencodedSer(err: serde_urlencoded::ser::Error) {
            from()
            description("serde urlencoded ser error")
            display("Serde urlencoded ser error: {}", err)
            cause(err)
        }
        #[cfg(feature = "yaml")]
        SerdeYaml(err: serde_yaml::Error) {
            description("serde yaml error")
            display("Serde yaml error: {}", err)
            cause(err)
        }
        #[cfg(feature = "xml")]
        SerdeXml(err: serde_xml_rs::Error) {
            description("serde xml error")
            display("Serde xml error: {}", err)
            cause(err)
        }
        EmptyResponses {
            description("empty responses")
            display("Empty responses")
        }
        UnknownApolloConfigurationKind(kind: &'static str) {
            description("unknown apollo configuration kind")
            display("Unknown apollo configuration kind: {}", kind)
        }
        ApolloContentNotFound {
            description("apollo content not found")
            display("Apollo content not found")
        }
        ApolloConfigNotFound {
            description("apollo config not found")
            display("Apollo config not found")
        }
        ApolloServerError {
            description("apollo server error")
            display("Apollo server error")
        }
        ApolloNotModified {
            description("apollo not modified")
            display("Apollo not modified")
        }
        ApolloOtherError(code: StatusCode) {
            description("apollo other error")
            display("apollo other error, status code: {}", code)
        }
        ApolloListenTimeout {
            description("apollo listen timeout")
            display("Apollo listen timeout")
        }
    }
}

#[cfg(feature = "yaml")]
impl From<serde_yaml::Error> for ApolloClientError {
    fn from(err: serde_yaml::Error) -> ApolloClientError {
        ApolloClientError::SerdeYaml(err)
    }
}

#[cfg(feature = "xml")]
impl From<serde_xml_rs::Error> for ApolloClientError {
    fn from(err: serde_xml_rs::Error) -> ApolloClientError {
        ApolloClientError::SerdeXml(err)
    }
}

/// Canonicalize the namespace. Just add `.properties` to the end of the namespace which not end
/// with `.properties` or `.xml` or `.json` or `.yaml` or `.yml` or `.txt`.
///
/// # Examples
///
/// ```rust
/// use apollo_client::canonicalize_namespace;
/// assert_eq!(canonicalize_namespace("foo"), "foo.properties");
/// assert_eq!(canonicalize_namespace("foo.yaml"), "foo.yaml");
/// ```
pub fn canonicalize_namespace(namespace: &str) -> String {
    if namespace.ends_with(".properties")
        || namespace.ends_with(".xml")
        || namespace.ends_with(".json")
        || namespace.ends_with(".yaml")
        || namespace.ends_with(".yml")
        || namespace.ends_with(".txt")
    {
        namespace.to_string()
    } else {
        format!("{}.properties", namespace)
    }
}

/// Configuration of Apollo and api information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ClientConfig<S: AsRef<str>, V: AsRef<[S]>> {
    pub config_server_url: S,
    pub app_id: S,
    pub cluster_name: S,
    pub namespace_names: V,
    #[serde(default)]
    pub ip: Option<IpValue<S>>,
}

impl Default for ClientConfig<&'static str, &'static [&'static str]> {
    fn default() -> Self {
        Self {
            config_server_url: "http://localhost:8080",
            app_id: "",
            cluster_name: "default",
            namespace_names: &["application"],
            ip: Default::default(),
        }
    }
}

impl Default for ClientConfig<&'static str, Vec<&'static str>> {
    fn default() -> Self {
        let client_config: ClientConfig<&'static str, &'static [&'static str]> = Default::default();
        Self {
            config_server_url: client_config.config_server_url,
            app_id: client_config.app_id,
            cluster_name: client_config.cluster_name,
            namespace_names: client_config.namespace_names.to_owned(),
            ip: Default::default(),
        }
    }
}

impl Default for ClientConfig<String, Vec<String>> {
    fn default() -> Self {
        let client_config: ClientConfig<&'static str, Vec<&'static str>> = Default::default();
        Self {
            config_server_url: client_config.config_server_url.to_owned(),
            app_id: client_config.app_id.to_owned(),
            cluster_name: client_config.cluster_name.to_owned(),
            namespace_names: client_config
                .namespace_names
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ip: Default::default(),
        }
    }
}

/// Apollo config api `ip` param value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IpValue<S: AsRef<str>> {
    /// Get the hostname of the machine.
    #[cfg(feature = "host-name")]
    HostName,

    /// Get the first ip of the machine match the prefix, such as `^10\.2\.`.
    #[cfg(feature = "host-ip")]
    HostIpRegex(S),

    /// Specify your own IP address or other text.
    Custom(S),
}

impl<S: AsRef<str>> IpValue<S> {
    fn to_str(&self) -> &str {
        match self {
            #[cfg(feature = "host-name")]
            IpValue::HostName => {
                use lazy_static::lazy_static;

                lazy_static! {
                    static ref HOSSNAME: String = {
                        hostname::get()
                            .ok()
                            .and_then(|hostname| hostname.into_string().ok())
                            .unwrap_or("unknown".to_string())
                    };
                }
                &HOSSNAME
            }

            #[cfg(feature = "host-ip")]
            IpValue::HostIpRegex(regex) => {
                use lazy_static::lazy_static;
                use systemstat::data::IpAddr;
                use systemstat::platform::common::Platform;
                use systemstat::System;

                lazy_static! {
                    static ref ALL_ADDRS: Vec<String> = System::new()
                        .networks()
                        .ok()
                        .map(|networks| networks
                            .values()
                            .map(|network| network.addrs.iter().filter_map(|network_addr| {
                                match network_addr.addr {
                                    IpAddr::V4(addr) => Some(addr.to_string()),
                                    IpAddr::V6(addr) => Some(addr.to_string()),
                                    _ => None,
                                }
                            }))
                            .flatten()
                            .collect())
                        .unwrap_or(Vec::new());
                }

                let re = Regex::new(regex.as_ref()).expect("Parse regex of HostIpRegex failed");

                ALL_ADDRS
                    .iter()
                    .find(|addr| re.is_match(addr))
                    .map(|s| s.as_str())
                    .unwrap_or("127.0.0.1")
            }

            IpValue::Custom(s) => s.as_ref(),
        }
    }
}

/// For apollo config api response to transfer to your favorite type.
pub trait FromBodies: Sized {
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self;
}

impl FromBodies for ApolloClientResult<()> {
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self {
        bodies.into_iter().map(|item| item.map(|_| ())).collect()
    }
}

impl FromBodies for ApolloClientResult<String> {
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self {
        match bodies.into_iter().nth(0) {
            Some(item) => item,
            None => Err(ApolloClientError::EmptyResponses),
        }
    }
}

impl FromBodies for Vec<ApolloClientResult<String>> {
    #[inline]
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self {
        bodies
    }
}

impl FromBodies for ApolloClientResult<Vec<String>> {
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self {
        bodies.into_iter().collect()
    }
}

impl FromBodies for ApolloClientResult<Response> {
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self {
        bodies
            .into_iter()
            .nth(0)
            .ok_or(ApolloClientError::EmptyResponses)
            .and_then(|body| body.and_then(|body| serde_json::from_str(&body).map_err(Into::into)))
    }
}

impl FromBodies for Vec<ApolloClientResult<Response>> {
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self {
        bodies
            .into_iter()
            .map(|body| body.and_then(|body| serde_json::from_str(&body).map_err(Into::into)))
            .collect()
    }
}

impl FromBodies for ApolloClientResult<Vec<Response>> {
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self {
        <Vec<ApolloClientResult<Response>>>::from_bodies(bodies)
            .into_iter()
            .collect()
    }
}

impl FromBodies for ApolloClientResult<HashMap<String, Response>> {
    fn from_bodies(bodies: Vec<ApolloClientResult<String>>) -> Self {
        let responses = <ApolloClientResult<Vec<Response>>>::from_bodies(bodies)?;
        Ok(responses
            .into_iter()
            .map(|response| (response.namespace_name.clone(), response))
            .collect())
    }
}

/// Kind of a configuration namespace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfigurationKind {
    Properties,
    Xml,
    Json,
    Yaml,
    Txt,
}

impl ConfigurationKind {
    /// Infer the configuration namespace kind.
    pub fn infer_namespace_kind(namespace_name: &str) -> Self {
        if namespace_name.ends_with(".xml") {
            ConfigurationKind::Xml
        } else if namespace_name.ends_with(".json") {
            ConfigurationKind::Json
        } else if namespace_name.ends_with(".yml") || namespace_name.ends_with(".yaml") {
            ConfigurationKind::Yaml
        } else if namespace_name.ends_with(".txt") {
            ConfigurationKind::Txt
        } else {
            ConfigurationKind::Properties
        }
    }
}

impl Display for ConfigurationKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        Display::fmt(
            match self {
                ConfigurationKind::Properties => "properties",
                ConfigurationKind::Xml => "xml",
                ConfigurationKind::Json => "json",
                ConfigurationKind::Yaml => "yaml",
                ConfigurationKind::Txt => "txt",
            },
            f,
        )
    }
}

/// Apollo config api response.
#[derive(Debug, Deserialize)]
pub struct Response {
    #[serde(rename = "appId")]
    pub app_id: String,
    pub cluster: String,
    #[serde(rename = "namespaceName")]
    pub namespace_name: String,
    pub configurations: IndexMap<String, String>,
    #[serde(rename = "releaseKey")]
    pub release_key: String,
}

impl Response {
    /// Get the `configurations.content` field of the response.
    pub fn get_configurations_content(&self) -> ApolloClientResult<&str> {
        self.configurations
            .iter()
            .find_map(|(k, s)| {
                if k == "content" {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .ok_or(ApolloClientError::ApolloContentNotFound)
    }

    /// Infer the configuration namespace kind.
    pub fn infer_kind(&self) -> ConfigurationKind {
        let namespace_name = &self.namespace_name;

        if namespace_name.ends_with(".xml") {
            ConfigurationKind::Xml
        } else if namespace_name.ends_with(".json") {
            ConfigurationKind::Json
        } else if namespace_name.ends_with(".yml") || namespace_name.ends_with(".yaml") {
            ConfigurationKind::Yaml
        } else if namespace_name.ends_with(".txt") {
            ConfigurationKind::Txt
        } else {
            ConfigurationKind::Properties
        }
    }

    /// Deserialize the `configurations` field for `properties`, or `configurations.content` for
    /// other namespace kind, without wrapper.
    pub fn deserialize_configuration<T: DeserializeOwned>(&self) -> ApolloClientResult<T> {
        match self.infer_kind() {
            ConfigurationKind::Properties => {
                let object = serde_json::Value::Object(
                    self.configurations
                        .iter()
                        .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
                        .collect(),
                );
                Ok(serde_json::from_value(object)?)
            }
            ConfigurationKind::Json => {
                Ok(serde_json::from_str(self.get_configurations_content()?)?)
            }
            #[cfg(feature = "yaml")]
            ConfigurationKind::Yaml => {
                Ok(serde_yaml::from_str(self.get_configurations_content()?)?)
            }
            #[cfg(feature = "xml")]
            ConfigurationKind::Xml => {
                Ok(serde_xml_rs::from_str(self.get_configurations_content()?)?)
            }
            ConfigurationKind::Txt => {
                let value =
                    serde_json::Value::String(self.get_configurations_content()?.to_string());
                Ok(serde_json::from_value(value)?)
            }
            #[allow(unreachable_patterns)]
            k => panic!(
                "You have to enable feature `{}` for parsing this configuration kind.",
                k
            ),
        }
    }
}

type Notifications = Vec<Notification>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Notification {
    namespace_name: String,
    notification_id: i32,
}

fn initialize_notifications<S: AsRef<str>>(namespace_names: &[S]) -> Notifications {
    namespace_names
        .iter()
        .map(|namespace_name| Notification {
            namespace_name: {
                let mut namespace_name = namespace_name.as_ref();
                if namespace_name.ends_with(".properties") {
                    namespace_name = &namespace_name[..namespace_name.len() - ".properties".len()];
                }
                namespace_name.to_owned()
            },
            notification_id: -1,
        })
        .collect()
}

fn update_notifications(this: &mut Notifications, newer: Notifications) {
    for newer_item in newer {
        for this_item in this.iter_mut() {
            if this_item.namespace_name == newer_item.namespace_name {
                this_item.notification_id = newer_item.notification_id;
            }
        }
    }
}

/// Represents the apollo client.
pub struct Client<T: AsRef<str>, V: AsRef<[T]>> {
    client_config: ClientConfig<T, V>,
    notifications: Notifications,
}

impl<S: AsRef<str> + Display, V: AsRef<[S]>> Client<S, V> {
    /// New with the configuration of apollo and api parameters.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use apollo_client::{Client, ClientConfig};
    /// let client_config: ClientConfig<String, Vec<String>> = Default::default();
    /// let _ = Client::new(client_config);
    /// ```
    pub fn new(client_config: ClientConfig<S, V>) -> Self {
        let notifications = initialize_notifications(client_config.namespace_names.as_ref());
        Self {
            client_config,
            notifications,
        }
    }

    /// Request apollo config api, and return response of your favorite type.
    pub async fn request<T: FromBodies>(&self) -> T {
        self.request_with_extras_query(None).await
    }

    pub async fn request_with_extras_query<T: FromBodies>(
        &self,
        extras_query: Option<&[(&str, &str)]>,
    ) -> T {
        self.request_with_extras_query_and_namespaces(
            extras_query,
            &self.client_config.namespace_names,
        )
        .await
    }

    /// Request apollo config api, and return response of your favorite type, with extras query.
    pub async fn request_with_extras_query_and_namespaces<
        T: FromBodies,
        Ns: AsRef<str>,
        Nv: AsRef<[Ns]>,
    >(
        &self,
        extras_query: Option<&[(&str, &str)]>,
        namespace_names: Nv,
    ) -> T {
        let namespace_names = namespace_names.as_ref();
        let mut futures = Vec::with_capacity(namespace_names.len());
        for namespace_name in namespace_names {
            futures.push(async move {
                let url = self.get_config_url(namespace_name.as_ref(), None, extras_query);
                match url {
                    Ok(url) => {
                        log::debug!("Request apollo config api: {}", &url);
                        Self::request_bodies(&url).await
                    }
                    Err(e) => Err(e.into()),
                }
            });
        }
        let bodies = join_all(futures).await;
        log::trace!("Response apollo config data: {:?}", bodies);
        FromBodies::from_bodies(bodies)
    }

    async fn request_bodies(url: &str) -> ApolloClientResult<String> {
        let client = HttpClientBuilder::new()
            .version_negotiation(VersionNegotiation::http11())
            .dns_cache(DnsCache::Disable)
            .timeout(DEFAULT_CONFIG_TIMEOUT)
            .build()?;

        let mut response = client.get_async(url).await?;
        Self::handle_response_status(&response)?;
        let bodies = response.text_async().await?;
        Ok(bodies)
    }

    /// Request apollo notification api just once.
    /// Return the namespace names if ok.
    pub async fn listen_once(&mut self) -> ApolloClientResult<Vec<String>> {
        let client = HttpClientBuilder::new()
            .version_negotiation(VersionNegotiation::http11())
            .dns_cache(DnsCache::Disable)
            .timeout(DEFAULT_LISTEN_TIMEOUT + Duration::from_secs(10))
            .build()?;

        let url = self.get_listen_url(&self.notifications)?;
        log::debug!("Request apollo notifications api: {}", &url);

        let mut response =
            match select(client.get_async(url), Delay::new(DEFAULT_LISTEN_TIMEOUT)).await {
                Either::Left((response, ..)) => response?,
                Either::Right(_) => Err(ApolloClientError::ApolloListenTimeout)?,
            };

        Self::handle_response_status(&response)?;

        let bodies = response.text_async().await?;
        let notifications: Notifications = serde_json::from_str(&bodies)?;
        log::trace!(
            "Response apollo notifications bodies: {:?}",
            &self.notifications
        );

        let notify_namespaces = notifications
            .iter()
            .map(|notification| notification.namespace_name.clone())
            .collect();
        update_notifications(&mut self.notifications, notifications);

        Ok(notify_namespaces)
    }

    /// Loop and request apollo notification api, if there is a change of the namespaces, return
    /// the response of your favorite type, or [`ApolloClientError`] if there is something wrong.
    pub async fn listen_and_request<T: FromBodies>(&mut self) -> ApolloClientResult<T> {
        self.listen_and_request_with_extras_query(None).await
    }

    /// Loop and request apollo notification api, if there is a change of the namespaces, return
    /// the response of your favorite type, or [`ApolloClientError`] if there is something wrong.
    pub async fn listen_and_request_with_extras_query<T: FromBodies>(
        &mut self,
        extras_query: Option<&[(&str, &str)]>,
    ) -> ApolloClientResult<T> {
        loop {
            match self.listen_once().await {
                Ok(namespaces) => {
                    return Ok(self
                        .request_with_extras_query_and_namespaces(extras_query, &namespaces)
                        .await)
                }
                Err(ApolloClientError::ApolloNotModified) => {}
                Err(ApolloClientError::ApolloListenTimeout) => {}
                Err(e) => Err(e)?,
            }
        }
    }

    fn handle_response_status<T>(response: &http::Response<T>) -> ApolloClientResult<()> {
        let status = response.status();
        if !status.is_success() {
            match response.status() {
                StatusCode::NOT_MODIFIED => Err(ApolloClientError::ApolloNotModified)?,
                StatusCode::NOT_FOUND => Err(ApolloClientError::ApolloConfigNotFound)?,
                StatusCode::INTERNAL_SERVER_ERROR => Err(ApolloClientError::ApolloServerError)?,
                status => Err(ApolloClientError::ApolloOtherError(status))?,
            }
        }
        Ok(())
    }

    fn get_config_url(
        &self,
        namespace_name: &str,
        release_key: Option<&str>,
        extras_query: Option<&[(&str, &str)]>,
    ) -> Result<String, serde_urlencoded::ser::Error> {
        let mut query = Vec::new();
        if let Some(release_key) = release_key {
            query.push(("releaseKey", release_key));
        }
        if let Some(ip) = &self.client_config.ip {
            query.push(("ip", ip.to_str()));
        }
        if let Some(extras_query) = extras_query {
            for item in extras_query {
                query.push(item.to_owned());
            }
        }

        let mut query = serde_urlencoded::to_string(query)?;
        if !query.is_empty() {
            query.insert(0, '?');
        }

        Ok(format!(
            "{config_server_url}/configs/{app_id}/{cluster_name}/{namespace_name}{query}",
            config_server_url = self.client_config.config_server_url,
            app_id = self.client_config.app_id,
            cluster_name = self.client_config.cluster_name,
            namespace_name = namespace_name,
            query = query,
        ))
    }

    fn get_listen_url(&self, notifications: &Notifications) -> ApolloClientResult<String> {
        let notifications = if notifications.len() > 0 {
            let notifications = &[("notifications", serde_json::to_string(&notifications)?)];
            let mut notifications = serde_urlencoded::to_string(notifications)?;
            notifications.insert(0, '&');
            notifications
        } else {
            "".to_string()
        };

        Ok(format!(
            "{config_server_url}/notifications/v2?appId={app_id}&cluster={cluster_name}{notifications}",
            config_server_url = self.client_config.config_server_url,
            app_id = self.client_config.app_id,
            cluster_name = self.client_config.cluster_name,
            notifications = notifications,
        ))
    }
}
