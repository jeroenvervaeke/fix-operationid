# Fix OperationID

A tool to generate diffs and fix operation ID mismatches in MongoDB Atlas CLI code.

## How to Build

```bash
cargo build --release
```

## How to Generate Diffs

```bash
cargo run -- diff --before ./data/before.json --after ./data/after.json --output ./output/diff.json 
```

## How to Fix the Code

```bash
cargo run -- fix --operation-id-diff ./output/diff.json --cli-directory /Users/jeroen.vervaeke/git/github.com/mongodb/mongodb-atlas-cli/internal/ --go-sdk-version go.mongodb.org/atlas-sdk/v20250312007/admin
```
