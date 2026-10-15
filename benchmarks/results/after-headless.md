| phase (median wall time) | 100 comp. | 500 comp. | 2000 comp. | 5000 comp. |
|---|---:|---:|---:|---:|
| script compile (lex+parse+elaborate) | 1.2 ms | 5.8 ms | 25.6 ms | 65.5 ms |
| build project (apply IR + PCB sync + zone fill) | 2.1 ms | 9.8 ms | 43.8 ms | 161.8 ms |
| save project (atomic, with backup) | 2.7 ms | 11.9 ms | 6.4 ms | 16.9 ms |
| load project (parse + validate) | 1.2 ms | 5.2 ms | 27.0 ms | 72.7 ms |
| schematic connectivity | 0.2 ms | 1.1 ms | 5.8 ms | 20.2 ms |
| ERC | 0.2 ms | 0.9 ms | 4.6 ms | 18.8 ms |
| DRC | 0.8 ms | 5.2 ms | 37.0 ms | 166.2 ms |
| ratsnest (full) | 0.1 ms | 0.4 ms | 2.1 ms | 5.9 ms |
| ratsnest (move one footprint, incremental) | 0.0 ms | 0.0 ms | 0.2 ms | 0.4 ms |
| zone refill (all zones) | 1.0 ms | 3.8 ms | 19.7 ms | 90.6 ms |
| router obstacle collection | 0.9 ms | 4.2 ms | 17.5 ms | 45.2 ms |
| router walk-around (20 routes) | 520.2 ms | 91.7 ms | 91.3 ms | 103.4 ms |
| manufacturing (preflight + Gerber + drill + BOM + zip) | 1.4 ms | 8.0 ms | 49.4 ms | 202.2 ms |
| **peak memory (RSS high-water)** | 9 MB | 18 MB | 45 MB | 108 MB |
