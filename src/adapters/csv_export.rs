//! CSV rendering of scan results, with the same columns as `scan.py`.

use crate::domain::ScanRow;

use super::dto::scan_row_to_dto;

/// Renders the rows as CSV text with a header line.
pub fn scan_rows_csv(rows: &[ScanRow]) -> Result<String, csv::Error> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for row in rows {
        writer.serialize(scan_row_to_dto(row))?;
    }
    let bytes = writer.into_inner().map_err(|e| e.into_error())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
