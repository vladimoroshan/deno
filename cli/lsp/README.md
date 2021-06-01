# Deno Language Server

The Deno Language Server provides a server implementation of the
[Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
which is specifically tailored to provide a _Deno_ view of code. It is
integrated into the command line and can be started via the `lsp` sub-command.

> :warning: The Language Server is experimental and not feature complete. This
> document gives an overview of the structure of the language server.

## Structure

When the language server is started, a `LanguageServer` instance is created
which holds all of the state of the language server. It also defines all of the
methods that the client calls via the Language Server RPC protocol.

## Settings

There are several settings that the language server supports for a workspace:

- `deno.enable`
- `deno.config`
- `deno.importMap`
- `deno.codeLens.implementations`
- `deno.codeLens.references`
- `deno.codeLens.referencesAllFunctions`
- `deno.suggest.completeFunctionCalls`
- `deno.suggest.names`
- `deno.suggest.paths`
- `deno.suggest.autoImports`
- `deno.suggest.imports.autoDiscover`
- `deno.suggest.imports.hosts`
- `deno.lint`
- `deno.unstable`

There are settings that are support on a per resource basis by the language
server:

- `deno.enable`

There are several points in the process where Deno analyzes these settings.
First, when the `initialize` request from the client, the
`initializationOptions` will be assumed to be an object that represents the
`deno` namespace of options. For example, the following value:

```json
{
  "enable": true,
  "unstable": true
}
```

Would enable Deno with the unstable APIs for this instance of the language
server.

When the language server receives a `workspace/didChangeConfiguration`
notification, it will assess if the client has indicated if it has a
`workspaceConfiguration` capability. If it does, it will send a
`workspace/configuration` request which will include a request for the workspace
configuration as well as the configuration of all URIs that the language server
is currently tracking.

If the client has the `workspaceConfiguration` capability, the language server
will send a configuration request for the URI when it received the
`textDocument/didOpen` notification in order to get the resources specific
settings.

If the client does not have the `workspaceConfiguration` capability, the
language server will assume the workspace setting applies to all resources.

## Custom requests

The LSP currently supports the following custom requests. A client should
implement these in order to have a fully functioning client that integrates well
with Deno:

- `deno/cache` - This command will instruct Deno to attempt to cache a module
  and all of its dependencies. If a `referrer` only is passed, then all
  dependencies for the module specifier will be loaded. If there are values in
  the `uris`, then only those `uris` will be cached.

  It expects parameters of:

  ```ts
  interface CacheParams {
    referrer: TextDocumentIdentifier;
    uris: TextDocumentIdentifier[];
  }
  ```
- `deno/performance` - Requests the return of the timing averages for the
  internal instrumentation of Deno.

  It does not expect any parameters.
- `deno/reloadImportRegistries` - Reloads any cached responses from import
  registries.

  It does not expect any parameters.
- `deno/virtualTextDocument` - Requests a virtual text document from the LSP,
  which is a read only document that can be displayed in the client. This allows
  clients to access documents in the Deno cache, like remote modules and
  TypeScript library files built into Deno. The Deno language server will encode
  all internal files under the custom schema `deno:`, so clients should route
  all requests for the `deno:` schema back to the `deno/virtualTextDocument`
  API.

  It also supports a special URL of `deno:/status.md` which provides a markdown
  formatted text document that contains details about the status of the LSP for
  display to a user.

  It expects parameters of:

  ```ts
  interface VirtualTextDocumentParams {
    textDocument: TextDocumentIdentifier;
  }
  ```

## Custom notifications

There is currently one custom notification that is send from the server to the
client:

- `deno/registryStatus` - when `deno.suggest.imports.autoDiscover` is `true` and
  an origin for an import being added to a document is not explicitly set in
  `deno.suggest.imports.hosts`, the origin will be checked and the notification
  will be sent to the client of the status.

  When receiving the notification, if the param `suggestion` is `true`, the
  client should offer the user the choice to enable the origin and add it to the
  configuration for `deno.suggest.imports.hosts`. If `suggestion` is `false` the
  client should add it to the configuration of as `false` to stop the language
  server from attempting to detect if suggestions are supported.

  The params for the notification are:

  ```ts
  interface RegistryStatusNotificationParams {
    origin: string;
    suggestions: boolean;
  }
  ```
