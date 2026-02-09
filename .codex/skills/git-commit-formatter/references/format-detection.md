# Format Detection Cheatsheet

Use this file only when repository conventions are unclear.

## Fast checks
- Conventional Commit config: `commitlint.config.js`, `.commitlintrc`, `@commitlint/*`
- Commitizen/cz: `.czrc`, `config.commitizen`
- Monorepo scopes: workspace/package names from `package.json`, `Cargo.toml`
- Ticket footer style: inspect last 20 commits for `Refs:`, `Closes:`, `(#123)`

## Conventional Commit baseline
- Header: `type(scope): subject`
- Breaking changes: add `!` in header or `BREAKING CHANGE:` in footer
- Subject style: imperative, concise, no period

## Common anti-patterns to rewrite
- Vague: `update stuff`
- Mixed concerns: feature + refactor + docs in one message
- Implementation dump: listing every file instead of intent
