use pyo3::prelude::*;

/// Engine version, re-exported from `blokus_core`.
#[pyfunction]
fn version() -> &'static str {
    blokus_core::version()
}

#[pymodule]
fn blokus(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}
