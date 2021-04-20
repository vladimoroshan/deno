// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use deno_core::error::generic_error;
use deno_core::error::type_error;
use deno_core::error::uri_error;
use deno_core::error::AnyError;
use deno_core::url::form_urlencoded;
use deno_core::url::quirks;
use deno_core::url::Url;
use deno_core::JsRuntime;
use deno_core::ZeroCopyBuf;
use serde::Deserialize;
use serde::Serialize;
use std::panic::catch_unwind;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlParseArgs {
  href: String,
  base_href: Option<String>,
  // If one of the following are present, this is a setter call. Apply the
  // proper `Url::set_*()` method after (re)parsing `href`.
  set_hash: Option<String>,
  set_host: Option<String>,
  set_hostname: Option<String>,
  set_password: Option<String>,
  set_pathname: Option<String>,
  set_port: Option<String>,
  set_protocol: Option<String>,
  set_search: Option<String>,
  set_username: Option<String>,
}

#[derive(Serialize)]
pub struct UrlParts {
  href: String,
  hash: String,
  host: String,
  hostname: String,
  origin: String,
  password: String,
  pathname: String,
  port: String,
  protocol: String,
  search: String,
  username: String,
}

/// Parse `UrlParseArgs::href` with an optional `UrlParseArgs::base_href`, or an
/// optional part to "set" after parsing. Return `UrlParts`.
pub fn op_url_parse(
  _state: &mut deno_core::OpState,
  args: UrlParseArgs,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<UrlParts, AnyError> {
  let base_url = args
    .base_href
    .as_ref()
    .map(|b| Url::parse(b).map_err(|_| type_error("Invalid base URL")))
    .transpose()?;
  let mut url = Url::options()
    .base_url(base_url.as_ref())
    .parse(&args.href)
    .map_err(|_| type_error("Invalid URL"))?;

  if let Some(hash) = args.set_hash.as_ref() {
    quirks::set_hash(&mut url, hash);
  } else if let Some(host) = args.set_host.as_ref() {
    quirks::set_host(&mut url, host).map_err(|_| uri_error("Invalid host"))?;
  } else if let Some(hostname) = args.set_hostname.as_ref() {
    quirks::set_hostname(&mut url, hostname)
      .map_err(|_| uri_error("Invalid hostname"))?;
  } else if let Some(password) = args.set_password.as_ref() {
    quirks::set_password(&mut url, password)
      .map_err(|_| uri_error("Invalid password"))?;
  } else if let Some(pathname) = args.set_pathname.as_ref() {
    quirks::set_pathname(&mut url, pathname);
  } else if let Some(port) = args.set_port.as_ref() {
    quirks::set_port(&mut url, port).map_err(|_| uri_error("Invalid port"))?;
  } else if let Some(protocol) = args.set_protocol.as_ref() {
    quirks::set_protocol(&mut url, protocol)
      .map_err(|_| uri_error("Invalid protocol"))?;
  } else if let Some(search) = args.set_search.as_ref() {
    quirks::set_search(&mut url, search);
  } else if let Some(username) = args.set_username.as_ref() {
    quirks::set_username(&mut url, username)
      .map_err(|_| uri_error("Invalid username"))?;
  }

  // TODO(nayeemrmn): Panic that occurs in rust-url for the `non-spec:`
  // url-constructor wpt tests: https://github.com/servo/rust-url/issues/670.
  let username = catch_unwind(|| quirks::username(&url)).map_err(|_| {
    generic_error(format!(
      "Internal error while parsing \"{}\"{}, \
       see https://github.com/servo/rust-url/issues/670",
      args.href,
      args
        .base_href
        .map(|b| format!(" against \"{}\"", b))
        .unwrap_or_default()
    ))
  })?;
  Ok(UrlParts {
    href: quirks::href(&url).to_string(),
    hash: quirks::hash(&url).to_string(),
    host: quirks::host(&url).to_string(),
    hostname: quirks::hostname(&url).to_string(),
    origin: quirks::origin(&url),
    password: quirks::password(&url).to_string(),
    pathname: quirks::pathname(&url).to_string(),
    port: quirks::port(&url).to_string(),
    protocol: quirks::protocol(&url).to_string(),
    search: quirks::search(&url).to_string(),
    username: username.to_string(),
  })
}

pub fn op_url_parse_search_params(
  _state: &mut deno_core::OpState,
  args: Option<String>,
  zero_copy: Option<ZeroCopyBuf>,
) -> Result<Vec<(String, String)>, AnyError> {
  let params = match (args, zero_copy) {
    (None, Some(zero_copy)) => form_urlencoded::parse(&zero_copy)
      .into_iter()
      .map(|(k, v)| (k.as_ref().to_owned(), v.as_ref().to_owned()))
      .collect(),
    (Some(args), None) => form_urlencoded::parse(args.as_bytes())
      .into_iter()
      .map(|(k, v)| (k.as_ref().to_owned(), v.as_ref().to_owned()))
      .collect(),
    _ => return Err(type_error("invalid parameters")),
  };
  Ok(params)
}

pub fn op_url_stringify_search_params(
  _state: &mut deno_core::OpState,
  args: Vec<(String, String)>,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<String, AnyError> {
  let search = form_urlencoded::Serializer::new(String::new())
    .extend_pairs(args)
    .finish();
  Ok(search)
}

/// Load and execute the javascript code.
pub fn init(isolate: &mut JsRuntime) {
  let files = vec![("deno:op_crates/url/00_url.js", include_str!("00_url.js"))];
  for (url, source_code) in files {
    isolate.execute(url, source_code).unwrap();
  }
}

pub fn get_declaration() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lib.deno_url.d.ts")
}
