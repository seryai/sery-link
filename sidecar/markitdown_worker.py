#!/usr/bin/env python3
"""
MarkItDown sidecar worker for Sery Link.

Reads a file path from stdin, converts it to Markdown using MarkItDown,
and writes the result to stdout as JSON.

Usage:
    echo "/path/to/document.docx" | python markitdown_worker.py

Output format:
    {"success": true, "markdown": "...", "error": null}
    {"success": false, "markdown": null, "error": "error message"}
"""
import sys
import json
from pathlib import Path

try:
    from markitdown import MarkItDown
except ImportError:
    # Fallback error if MarkItDown not installed
    json.dump({
        "success": False,
        "markdown": None,
        "error": "MarkItDown not installed. Run: pip install markitdown[all]"
    }, sys.stdout)
    sys.exit(1)


def convert_document(file_path: str) -> dict:
    """Convert a document to Markdown."""
    try:
        # Validate file exists
        path = Path(file_path)
        if not path.exists():
            return {
                "success": False,
                "markdown": None,
                "error": f"File not found: {file_path}"
            }

        # Convert with MarkItDown
        md = MarkItDown()
        result = md.convert(str(path))

        if result and result.text_content:
            return {
                "success": True,
                "markdown": result.text_content,
                "error": None
            }
        else:
            return {
                "success": False,
                "markdown": None,
                "error": "MarkItDown returned no content"
            }

    except Exception as e:
        return {
            "success": False,
            "markdown": None,
            "error": f"Conversion failed: {str(e)}"
        }


def main():
    """Main entry point - read from stdin, write to stdout."""
    # Read file path from stdin
    file_path = sys.stdin.read().strip()

    if not file_path:
        result = {
            "success": False,
            "markdown": None,
            "error": "No file path provided on stdin"
        }
    else:
        result = convert_document(file_path)

    # Write JSON result to stdout
    json.dump(result, sys.stdout)
    sys.stdout.flush()


if __name__ == "__main__":
    main()
