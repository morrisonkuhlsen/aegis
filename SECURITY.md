# Security policy

## Supported versions

MK aeGIS is in alpha (0.x). Security fixes go into the next release; only the latest release is
supported.

## Reporting a vulnerability

Report it **privately**: on GitHub, open the repository's **Security** tab and choose **Report a
vulnerability** (a GitHub Security Advisory). Please don't open a public issue.

Include what you can: the version (`mkaegis-cli --version`), the platform, the input or the steps
that trigger it, and what an attacker gains. You'll get an answer within a week. Once a fix is
released, the advisory is published with credit to you, unless you'd rather not be named.

## What counts

MK aeGIS opens files people receive from others, so a file must never be able to do harm:

- a shapefile, GeoJSON, `.aegis` project, `.prj`, `.cpg`, GAL or GWT file that crashes the
  program, hangs it, exhausts memory, or writes outside the paths the person chose;
- a `WHERE` clause or command parameter that does the same;
- the MCP server (`mkaegis-cli mcp`) doing more than the commands it exposes.

A crash on a hostile input is a bug we treat as a security issue (the project's "never crash"
rule): report it privately if you think it can be exploited, as a normal issue otherwise.

## By design

- **The control channel** (`mkaegis --control PORT`) listens on `127.0.0.1` without
  authentication, so any program on the same machine can run commands, including ones that write
  files, and take screenshots. It is off unless started with `--control`; use it only on a machine
  you trust. See [`docs/control-protocol.md`](docs/control-protocol.md).
- **The MCP server** runs the commands an agent sends it, with the rights of the user who started
  it, as a command line does.
- **Release binaries are not code-signed yet**; check them against `SHA256SUMS.txt` on the
  release page.
