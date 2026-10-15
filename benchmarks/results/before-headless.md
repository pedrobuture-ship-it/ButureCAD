| phase (median wall time) | 100 comp. | 500 comp. | 2000 comp. | 5000 comp. |
|---|---:|---:|---:|---:|
| script compile (lex+parse+elaborate) | 1.2 ms | 5.7 ms | 23.8 ms | 64.4 ms |
| build project (apply IR + PCB sync + zone fill) | 2.6 ms | 24.4 ms | 286.8 ms | 1.78 s |
| save project (atomic, with backup) | 2.6 ms | 11.9 ms | 6.6 ms | 16.5 ms |
| load project (parse + validate) | 1.1 ms | 5.1 ms | 26.4 ms | 70.5 ms |
| schematic connectivity | 0.2 ms | 1.1 ms | 5.7 ms | 18.5 ms |
| ERC | 0.2 ms | 0.9 ms | 4.9 ms | 18.6 ms |
| DRC | 0.8 ms | 4.8 ms | 37.1 ms | 157.4 ms |
| ratsnest (full) | 0.1 ms | 0.4 ms | 2.3 ms | 6.3 ms |
| ratsnest (move one footprint, incremental) | 0.0 ms | 0.0 ms | 0.2 ms | 0.4 ms |
| zone refill (all zones) | 0.8 ms | 3.9 ms | 20.6 ms | 88.7 ms |
| router obstacle collection | 0.9 ms | 4.2 ms | 18.4 ms | 46.4 ms |
| router walk-around (20 routes) | 520.6 ms | 93.8 ms | 93.8 ms | 97.0 ms |
| manufacturing (preflight + Gerber + drill + BOM + zip) | 1.4 ms | 7.9 ms | 48.9 ms | 194.3 ms |
| **peak memory (RSS high-water)** | 9 MB | 18 MB | 45 MB | 107 MB |
