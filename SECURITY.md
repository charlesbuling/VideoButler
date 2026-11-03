# Security policy

Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>

## Reporting

Please report a security issue by email to **217010072@link.cuhk.edu.cn**.
Do not open a public GitHub issue for a vulnerability, and do not post
an exploit where other people can run it before a fix exists.

Include what you can of the following:

- the version or commit (`0.2.0` / the `macos-app` revision);
- the platform (macOS, iOS, Android, Linux);
- what an attacker can do, and what they already need;
- a description that lets the maintainer reproduce the issue.

The maintainer will reply when they have read the report. There is no
bug-bounty program.

## Scope

Video Butler is an offline, on-device editor. The network code lives in
`vb-cloud` (model downloads and an optional OpenAI-compatible endpoint).
`vb serve` binds loopback and sends no CORS headers; binding it to a
public interface or adding permissive CORS would be a vulnerability.

In scope: the engine, the app, the local HTTP API, draft export, and
model download. Out of scope: a third-party model or ffmpeg vulnerability
with no path through this project, and a person who already has an
account on the machine reading files that account can read.

## Supported versions

| Version | Branch | Security fixes |
|---|---|---|
| 0.2.0 | `macos-app` | the current line |

Older tags are not maintained as a separate supported series.
The Windows tree (`charlesbuling/VideoBulter-windows`) is a separate
repository and is not part of this 0.2.0 release.
