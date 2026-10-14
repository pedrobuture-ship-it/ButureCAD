//! What a simulation returns.

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// `v(out)`, `i(V1)`, `mag(v(out))`, …
    pub name: String,
    /// `V`, `A`, `dB`, `deg`.
    pub unit: &'static str,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SimResult {
    pub analysis: String,
    pub backend: String,
    /// Label of the x axis (`time`, `frequency`, `v1`), empty for an operating point.
    pub x_label: String,
    pub x_unit: &'static str,
    pub x_log: bool,
    pub x: Vec<f64>,
    pub series: Vec<Series>,
}

impl SimResult {
    pub fn series(&self, name: &str) -> Option<&Series> {
        self.series
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Operating point: one value per series.
    pub fn scalar(&self, name: &str) -> Option<f64> {
        self.series(name).and_then(|s| s.values.first().copied())
    }
}
