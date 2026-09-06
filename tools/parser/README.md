# Parser

A CLI tool for converting `.tmj` (Tiled JSON map) files into a JSON format ready for backend processing.

## Overview

The parser works in two steps:

1. **`init`** — generates a configuration file describing the source map and which tiles count as roads.
2. **`parse`** — reads the configuration, converts the referenced `.tmj` map into a backend-ready JSON file.

## Usage

### 1. Initialize configuration

```bash
./parser init
```

This creates a `config.json` file in the current directory with the following structure:

```json
{
  "map_path": "",
  "roads": []
}
```

| Field | Type | Description |
|---|---|---|
| `map_path` | `String` | Path to the source `.tmj` map file to be parsed. |
| `roads` | `Vec<i64>` | Array of tile IDs that represent road cells, used by the pathfinding algorithm. |

After running `init`, open `config.json` and fill in the fields manually. For example:

```json
{
  "map_path": "./assets/map.tmj",
  "roads": [
    0,
    1,
    18,
    8,
    19
  ]
}
```

> **Note:** `init` **WILL OVERRIDE** an existing `config.json`. If a configuration file is already present in the current directory, `init` will erase that.

### 2. Parse the map

```bash
./parser parse
```

Reads `config.json` from the current directory, loads the `.tmj` file at `map_path`, and produces a parsed JSON file ready for consumption by the backend.

## Requirements

- A valid `config.json` must exist in the current directory before running `parse` (create one with `init` first).
- `map_path` must point to an existing, valid `.tmj` file.
- `roads` must list the tile IDs that should be treated as traversable road cells; tiles not listed here are treated as non-road terrain by the pathfinding algorithm.

## Typical workflow

```bash
# 1. Generate the config template
./parser init

# 2. Edit config.json — set map_path and roads
#    (see example above)

# 3. Convert the map
./parser parse
```

## Output

`parse` produces a JSON file derived from the source `.tmj` map, structured for direct use by the backend's map-loading and pathfinding logic (see `parser::parse_from_json` on the backend side).
