# Contributor credits

*Updated 2026-10-21.*

*Help › About MK aeGIS* lists the people and the AI models that built MK aeGIS, compiled into the
binary from two files in [`contributors/`](../contributors/) (`crates/ui-egui/src/credits.rs`):

| File                | What it holds                                            | Who edits it                          |
|---------------------|----------------------------------------------------------|---------------------------------------|
| `contributors.json` | Commits per person (by GitHub username) and per AI model | Generated: `cargo xtask contributors` |
| `people.toml`       | Names people chose for themselves                        | Each person, for their own entry      |

## How people are counted

`cargo xtask contributors` reads `git log` (merge commits excluded). A person is their GitHub
username, taken from their `…@users.noreply.github.com` address, or else their author name. A
model is counted from a `Co-Authored-By:` trailer naming Claude (or an `@anthropic.com` noreply
address). `cargo xtask contributors --check` only verifies that the file is current; maintainers
refresh it, and nobody edits it by hand.

## Choosing your name

Without an entry you are credited as `@username`. To be shown by another name, add **your own**
entry to `people.toml`, in your own pull request, committed as you:

```toml
[people.your-github-username]
real_name = "Your Name"                     # optional
display_name = "How you'd like to appear"   # optional; wins over real_name
sync_github_name = false                    # true: use your public GitHub profile name
```

Never add, edit or guess anyone else's entry, not from git config, commit authors or GitHub
profiles. An agent working for you asks you once, then adds only your entry.
