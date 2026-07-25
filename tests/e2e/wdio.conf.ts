// WebdriverIO config for the tauri-driver smoke test (quickstart.md, T049).
//
// NOT verified in CI/sandbox yet — tauri-driver needs a real display server
// (and the platform's native WebDriver: WebView2 driver on Windows,
// WebKitWebDriver on Linux, none needed on macOS since it drives WKWebView
// directly) which this development environment does not have.
//
// One-time setup before this can run:
//   cargo install tauri-driver
//   pnpm add -D @wdio/cli @wdio/local-runner @wdio/mocha-framework @wdio/types
//
// Then, to run:
//   cargo tauri build --debug
//   tauri-driver &
//   pnpm exec wdio run tests/e2e/wdio.conf.ts
import type { Options } from "@wdio/types";

const config: Options.Testrunner = {
  runner: "local",
  specs: ["./tests/e2e/*.spec.ts"],
  maxInstances: 1,
  hostname: "127.0.0.1",
  port: 4444,
  path: "/",
  capabilities: [
    {
      "tauri:options": {
        application: "../../src-tauri/target/debug/media-downloader",
      },
    } as WebdriverIO.Capabilities,
  ],
  logLevel: "info",
  framework: "mocha",
  mochaOpts: {
    ui: "bdd",
    timeout: 120_000,
  },
};

export { config };
