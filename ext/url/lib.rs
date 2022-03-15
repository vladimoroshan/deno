// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

mod urlpattern;

use deno_core::error::generic_error;
use deno_core::error::type_error;
use deno_core::error::uri_error;
use deno_core::error::AnyError;
use deno_core::include_js_files;
use deno_core::op;
use deno_core::url::form_urlencoded;
use deno_core::url::quirks;
use deno_core::url::Url;
use deno_core::Extension;
use deno_core::ZeroCopyBuf;
use std::panic::catch_unwind;

use crate::urlpattern::op_urlpattern_parse;
use crate::urlpattern::op_urlpattern_process_match_input;

pub fn init() -> Extension {
  Extension::builder()
    .js(include_js_files!(
      prefix "deno:ext/url",
      "00_url.js",
      "01_urlpattern.js",
    ))
    .ops(vec![
      op_url_parse::decl(),
      op_url_reparse::decl(),
      op_url_parse_search_params::decl(),
      op_url_stringify_search_params::decl(),
      op_urlpattern_parse::decl(),
      op_urlpattern_process_match_input::decl(),
    ])
    .build()
}

// UrlParts is a \n joined string of the following parts:
// #[derive(Serialize)]
// pub struct UrlParts {
//   href: String,
//   hash: String,
//   host: String,
//   hostname: String,
//   origin: String,
//   password: String,
//   pathname: String,
//   port: String,
//   protocol: String,
//   search: String,
//   username: String,
// }
// TODO: implement cleaner & faster serialization
type UrlParts = String;

/// Parse `UrlParseArgs::href` with an optional `UrlParseArgs::base_href`, or an
/// optional part to "set" after parsing. Return `UrlParts`.
#[op]
pub fn op_url_parse(
  href: String,
  base_href: Option<String>,
) -> Result<UrlParts, AnyError> {
  let base_url = base_href
    .as_ref()
    .map(|b| Url::parse(b).map_err(|_| type_error("Invalid base URL")))
    .transpose()?;
  let url = Url::options()
    .base_url(base_url.as_ref())
    .parse(&href)
    .map_err(|_| type_error("Invalid URL"))?;

  url_result(url, href, base_href)
}

#[derive(
  serde_repr::Serialize_repr, serde_repr::Deserialize_repr, PartialEq, Debug,
)]
#[repr(u8)]
pub enum UrlSetter {
  Hash = 1,
  Host = 2,
  Hostname = 3,
  Password = 4,
  Pathname = 5,
  Port = 6,
  Protocol = 7,
  Search = 8,
  Username = 9,
}

#[op]
pub fn op_url_reparse(
  href: String,
  setter_opts: (UrlSetter, String),
) -> Result<UrlParts, AnyError> {
  let mut url = Url::options()
    .parse(&href)
    .map_err(|_| type_error("Invalid URL"))?;

  let (setter, setter_value) = setter_opts;
  let value = setter_value.as_ref();

  match setter {
    UrlSetter::Hash => quirks::set_hash(&mut url, value),
    UrlSetter::Host => quirks::set_host(&mut url, value)
      .map_err(|_| uri_error("Invalid host"))?,
    UrlSetter::Hostname => quirks::set_hostname(&mut url, value)
      .map_err(|_| uri_error("Invalid hostname"))?,
    UrlSetter::Password => quirks::set_password(&mut url, value)
      .map_err(|_| uri_error("Invalid password"))?,
    UrlSetter::Pathname => quirks::set_pathname(&mut url, value),
    UrlSetter::Port => quirks::set_port(&mut url, value)
      .map_err(|_| uri_error("Invalid port"))?,
    UrlSetter::Protocol => quirks::set_protocol(&mut url, value)
      .map_err(|_| uri_error("Invalid protocol"))?,
    UrlSetter::Search => quirks::set_search(&mut url, value),
    UrlSetter::Username => quirks::set_username(&mut url, value)
      .map_err(|_| uri_error("Invalid username"))?,
  }

  url_result(url, href, None)
}

fn url_result(
  url: Url,
  href: String,
  base_href: Option<String>,
) -> Result<UrlParts, AnyError> {
  // TODO(nayeemrmn): Panic that occurs in rust-url for the `non-spec:`
  // url-constructor wpt tests: https://github.com/servo/rust-url/issues/670.
  let username = catch_unwind(|| quirks::username(&url)).map_err(|_| {
    generic_error(format!(
      "Internal error while parsing \"{}\"{}, \
       see https://github.com/servo/rust-url/issues/670",
      href,
      base_href
        .map(|b| format!(" against \"{}\"", b))
        .unwrap_or_default()
    ))
  })?;

  Ok(
    [
      quirks::href(&url),
      quirks::hash(&url),
      quirks::host(&url),
      quirks::hostname(&url),
      &quirks::origin(&url),
      quirks::password(&url),
      quirks::pathname(&url),
      quirks::port(&url),
      quirks::protocol(&url),
      quirks::search(&url),
      username,
    ]
    .join("\n"),
  )
}

#[op]
pub fn op_url_parse_search_params(
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

#[op]
pub fn op_url_stringify_search_params(
  args: Vec<(String, String)>,
) -> Result<String, AnyError> {
  let search = form_urlencoded::Serializer::new(String::new())
    .extend_pairs(args)
    .finish();
  Ok(search)
}
