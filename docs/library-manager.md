# Library Manager (roadmap stage 31)

Tab **Libraries** (`Ctrl+7`). Fully local: libraries are JSON files in the **project** folder
(`<project>/libraries`) and the **user** folder; nothing is downloaded and the datasheet field is a
stored path/URL that is never opened or fetched.

## Library types

| Type | File | Holds |
|---|---|---|
| Symbol libraries | `*.bsymlib.json` (stage 09) | symbols |
| Footprint libraries | `*.bfplib.json` (stage 12) | footprints (with their 3D model references) |
| 3D model libraries | `*.bmodlib.json` | a manifest of model files (name, path, description). Files stay where the user keeps them; they are not copied or parsed (STEP geometry: see `docs/mcad.md`). |
| Component libraries | `*.bcomplib.json` | components: symbol, footprint, 3D model, manufacturer, MPN, description, datasheet, parameters, tags |

All are written atomically, validated on load and save (names, lengths, control characters,
qualified `Library:Name` ids, size limit) and listed with their error when broken, so a bad file
is visible instead of silently lost. A component only *references* symbol/footprint/model; the
manager shows references that do not resolve.

## Search, filter, favorites, recents

`buturecad_libraries::Index` is an in-memory inverted index (sorted token map → postings) over all
library folders plus the built-in `Buture` symbols. A query is split into tokens and every token
must prefix-match a token of the item (name, library, description, manufacturer, MPN, tags,
parameters, kind). Results are ranked (exact name, name prefix, other) and can be filtered by kind,
library, manufacturer, favorites and recents. A unit test searches 60 000 items; the index is
rebuilt on *Refresh* and after every library edit (it is not stored on disk).
Favorites and the 20 most recent items are saved in `<user libraries>/.library-prefs.json`; without
a user folder they live only in the session (the panel says so).

## Using it

Search box and filters on top; click a result for its details; ★ toggles favorite. *New component…*
and *Add 3D model…* write to the project or user folder (`Buture` is read-only). *Place symbol*
arms the schematic placement tool with the item's symbol.

## Limits

* Placing a component places its **symbol only**; footprint, value and manufacturer data are not
  yet copied into the schematic part.
* No import/export of third-party libraries, no library versioning, no duplicate-detection across
  libraries, no thumbnails/previews of symbols or footprints in the list.
* Index rebuild is a full re-read (fast for thousands of items, not incremental).
