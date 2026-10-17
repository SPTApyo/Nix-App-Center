---
name: e2e-step
description: Add or fix a step of the Nix App Center end-to-end scenario (tests/e2e). Use when asked to test a UI flow end to end, or when an e2e step breaks after a UI change.
---

# Add an end-to-end step

Read docs/testing.md first. The suite clicks by coordinates on a Broadway
canvas, so every coordinate must be checked on a screenshot.

1. Build and serve an isolated app in the background:
   tests/e2e/run.sh --serve. Wait for the "e2e: serving" line.
2. Reach the screen under test with tests/e2e/session.py, one command batch
   at a time, and read every screenshot it writes before the next click.
3. When a click lands, name its coordinates with a constant at the top of
   tests/e2e/nix_app_center_e2e.py if it is reused.
4. Add the steps to scenario() or actions(). After each action, assert on
   what it must change: a file in env.config, env.kept(), env.tried(), or
   env.logged("pkexec") for the fake programs. A step without an assertion
   only proves the app did not crash.
5. Stop the served app, then run the whole suite with just e2e.
6. Report which steps you added and attach the relevant screenshots from
   target/e2e.

Never point the suite at the real configuration or the real data directory,
and never replace a fake program with the real one.
