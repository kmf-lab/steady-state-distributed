#!/bin/bash

# This script generates a summary of a Rust project within the specified root directory,
# including the project structure and the contents of all .rs and .toml files,
# excluding 'target' and hidden directories.
# The output is written to lesson-02C-distributed-runner.md in the original directory,
# with a .txt copy created if needed.
# Set the ROOT_DIR variable to the desired subdirectory, e.g., "runner" or "pod/publish".
# Run this script from the root of the Rust project using Bash.

# Define the root directory for the search
ROOT_DIR="runner"  # or "pod/publish", etc.

# Get the original directory
ORIGINAL_DIR=$(pwd)

# Define the output file in the original directory
OUTPUT_FILE="$ORIGINAL_DIR/lesson-02C-distributed-runner.md"

# Define the README path in the original directory
README_PATH="$ORIGINAL_DIR/README.md"

# Function to generate a directory tree
get_directory_tree() {
    local path=$1
    local prefix=$2
    local items=()
    # Collect items excluding 'target' and hidden directories
    for item in "$path"/*; do
        if [[ -e "$item" && $(basename "$item") != "target" && $(basename "$item") != .* ]]; then
            items+=("$item")
        fi
    done
    local count=${#items[@]}
    for ((i=0; i<count; i++)); do
        local item=${items[$i]}
        local last=$((i == count - 1))
        local item_prefix
        if $last; then
            item_prefix='\-- '
        else
            item_prefix='+-- '
        fi
        echo "$prefix$item_prefix$(basename "$item")"
        if [[ -d "$item" ]]; then
            if $last; then
                local new_prefix="$prefix    "
            else
                local new_prefix="$prefix|   "
            fi
                get_directory_tree "$item" "$new_prefix"
        fi
    done
}

# Initialize the output file with README.md content if it exists
if [ -f "$README_PATH" ]; then
    cat "$README_PATH" > "$OUTPUT_FILE"
    echo "" >> "$OUTPUT_FILE"
    echo "---" >> "$OUTPUT_FILE"
    echo "" >> "$OUTPUT_FILE"
else
    echo "" > "$OUTPUT_FILE"
fi

# Change to the specified root directory
pushd "$ROOT_DIR" > /dev/null

# Add the lesson title and project structure section
echo "# Lesson 02C: Distributed Runner" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"
echo "## Project Structure" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"
echo '```' >> "$OUTPUT_FILE"
echo "." >> "$OUTPUT_FILE"
get_directory_tree . '' >> "$OUTPUT_FILE"
echo '```' >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"

# Find all Cargo.toml files to identify Rust project directories
find . -name "Cargo.toml" -type f | while read -r cargo_toml; do
    project_dir=$(dirname "$cargo_toml")
    # Find all *.rs and *.toml files, excluding 'target' and hidden directories
    find "$project_dir" -type d \( -name "target" -o -name ".*" \) -prune -o -type f \( -name "*.rs" -o -name "*.toml" \) -print | while read -r file; do
        # Use the file path relative to the root directory
        relative_path="$file"
        # Read file content
        content=$(cat "$file")
        # Ensure content ends with a newline
        if ! echo "$content" | grep -q $'\n$'; then
            content+=$'\n'
        fi
        # Determine code block type based on file extension
        if [[ $file == *.rs ]]; then
            codeblock='```rust'
        elif [[ $file == *.toml ]]; then
            codeblock='```toml'
        else
            codeblock='```text'
        fi
        # Append file information to the output file
        echo "## $relative_path" >> "$OUTPUT_FILE"
        echo "" >> "$OUTPUT_FILE"
        echo "$codeblock" >> "$OUTPUT_FILE"
        echo "" >> "$OUTPUT_FILE"
        echo "$content" >> "$OUTPUT_FILE"
        echo '```' >> "$OUTPUT_FILE"
        echo "" >> "$OUTPUT_FILE"
    done
done

# Restore the original directory
popd > /dev/null

# Create a .txt copy if the output file doesn't end with .txt
if [[ ! "$OUTPUT_FILE" == *.txt ]]; then
    cp "$OUTPUT_FILE" "$OUTPUT_FILE.txt"
fi