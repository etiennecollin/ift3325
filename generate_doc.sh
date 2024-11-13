#!/usr/bin/env bash

# Check if arg -o is passed
if [ "$1" == "-o" ]; then
    cargo doc --workspace --document-private-items --no-deps --open
else
    cargo doc --workspace --document-private-items --no-deps
fi

rm -rf ./docs
mkdir -p ./docs
cp -r ./target/doc/* ./docs
echo "<meta http-equiv=\"refresh\" content=\"0; url=client/index.html\">" >./docs/index.html
