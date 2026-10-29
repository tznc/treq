# Remote Development acceptance traceability

This document maps the ship-blocking criteria in `prds/remote-development.md` to implementation tests.

The PRD intentionally defines user-visible shipping behavior rather than turning transport internals, provider quotas, certificate scheduling, audit plumbing, or test-resource cleanup into standalone product criteria.

## Test suites

Existing remote test suites continue to cover transport, control-plane, trust, lifecycle, mutation-safety, terminal, and UI behavior. Real-provider suites remain credential-gated and a skipped real-provider suite is not considered an acceptance pass.

## Acceptance criteria mapping

| # | Criterion | Existing evidence | Remaining acceptance gap |
|---|---|---|---|
| 1 | Managed setup works end-to-end | Managed provisioning/readiness tests, setup UI tests, native remote e2e harness, repository open/init commands | Run the complete user flow against the supported managed test environment, including returning to an existing repository |
| 2 | User-managed SSH works end-to-end | Host trust UI/unit tests, SSH transport tests, explicit endpoint registration structures, real-sshd integration harness | Full UI-to-real-host acceptance path |
| 3 | Remote repositories use the normal Treq workspace/review UI | Remote review refresh tests, query-key isolation, remote repository restore integration coverage | Full remote rendering of workspaces, diffs, commits, context, and conflicts |
| 4 | Core workspace mutations work remotely | Typed command construction, allow-list dispatch, structured error mapping, mutation verification recipes | End-to-end coverage of the complete set of mutations required by the normal Treq workflow |
| 5 | Agents work in remote workspaces | Agent remote command support and remote PTY/command infrastructure | End-to-end start/interact/reattach/stop of a coding agent on a provisioned remote environment |
| 6 | Interactive terminals work in remote workspaces | PTY cwd/I/O/resize/close tests, real-sshd PTY harness, native remote PTY test | Product-level detach/reattach flow across disconnect/app navigation |
| 7 | Reconnect is safe | Verify-before-retry transport tests and ambiguity handling | Acceptance coverage through a real mid-mutation network interruption |
| 8 | Out-of-band remote changes become visible | Change-marker refresh tests and direct-operation marker coverage | Product-level acceptance covering changes made from another client/agent while review UI is open |
| 9 | Trust failures fail closed | Host-key rejection, certificate cutoff/expiry/revocation, real-sshd trust tests | Run credential-gated trust paths in the supported acceptance environment |
| 10 | Normal managed lifecycle recovery works | Wake/suspend lifecycle tests, status banner tests, provider wake harness | Full suspend/wake/reconnect user flow against the supported managed provider |

## Supporting engineering coverage

The repository also contains tests for implementation requirements such as connection pooling, credential renewal, provider idempotency, resource configuration, audit redaction, observability, generation changes, and safe test-resource cleanup.

Those remain valuable engineering tests, but their existence does not add product acceptance criteria beyond the ten behaviors above.

## Real-environment testing

Credential-gated provider/Supabase/SSH suites must report skipped runs honestly. A harness that compiles and skips because credentials are absent is not evidence that the corresponding acceptance criterion passed.

Operational setup, environment variables, cleanup safety gates, and provider-specific run instructions belong with the test harness rather than in the product PRD.

There is no live managed-provider suite today. The Fly Sprites suites (`remote_e2e.rs`, `remote_e2e_native.rs`, `supabase/functions/tests/remote_e2e.test.ts`) were removed with the Sprites provider, which cannot run treq-gateway (`docs/design/sandbox-provider-adapter.md`). Boat and Daytona are covered by the Deno adapter and contract tests in `supabase/functions/tests/`, and the relay byte path by `remote_ssh_server_it.rs` against a real sshd.
