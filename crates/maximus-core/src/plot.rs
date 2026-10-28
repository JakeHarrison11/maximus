//! Backend-independent plot data (produced by sampling in Maxima, drawn by `maximus-plot`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Series {
    pub label: String,
    /// `None` breaks the line (undefined or non-real value).
    pub pts: Vec<Option<(f64, f64)>>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Grid3 {
    pub label: String,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub zs: Vec<Vec<Option<f64>>>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PlotData {
    /// `plot2d` or `plot3d`.
    pub kind: String,
    #[serde(default)]
    pub series: Vec<Series>,
    #[serde(default)]
    pub xlabel: String,
    #[serde(default)]
    pub ylabel: String,
    #[serde(default)]
    pub yrange: Option<(f64, f64)>,
    #[serde(default)]
    pub grid: Option<Grid3>,
}

impl PlotData {
    pub fn from_json(s: &str) -> Result<PlotData, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    pub fn is_3d(&self) -> bool {
        self.kind == "plot3d"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_driver_json() {
        let p = PlotData::from_json(r#"{"kind":"plot2d","series":[{"label":"x","pts":[[0.0,1.0],null,[2.0,3.0]]}],"xlabel":"x","yrange":null}"#).unwrap();
        assert_eq!(p.series[0].pts.len(), 3);
        assert_eq!(p.series[0].pts[1], None);
        let p = PlotData::from_json(r#"{"kind":"plot3d","series":[],"xlabel":"x","ylabel":"y","grid":{"label":"f","xs":[0,1],"ys":[0,1],"zs":[[0,null],[1,2]]}}"#).unwrap();
        assert!(p.is_3d());
        assert_eq!(p.grid.unwrap().zs[0][1], None);
    }
}
