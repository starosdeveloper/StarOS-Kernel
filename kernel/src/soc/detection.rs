use crate::error::{KernelError, Result};
use crate::prelude::*;
use crate::devicetree::parser::FdtParser;
use crate::devicetree::properties::{parse_string, parse_stringlist};

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]

#[cfg(feature = "std")]
use std::{string::String, vec::Vec, format};

#[derive(Debug, Clone, PartialEq)]
pub enum SocFamily {
    Qualcomm(SnapdragonModel),
    MediaTek(DimensityModel),
    Exynos(ExynosModel),
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SnapdragonModel {
    SD845,
    SD855,
    SD865,
    SD888,
    SD8Gen1,
    SD8Gen2,
    SD660,
    SD665,
    SD675,
    SD710,
    SD720G,
    SD730,
    SD750G,
    SD765G,
    SD778G,
    SD780G,
    SD695,
    SD480,
    SD460,
    SD439,
    SD429,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DimensityModel {
    D700,
    D720,
    D800,
    D810,
    D820,
    D900,
    D920,
    D1000,
    D1100,
    D1200,
    D8000,
    D8100,
    D8200,
    D9000,
    D9200,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExynosModel {
    E850,
    E880,
    E980,
    E990,
    E1080,
    E1280,
    E1330,
    E1380,
    E2100,
    E2200,
    Unknown(String),
}

pub struct SocInfo {
    pub family: SocFamily,
    pub model: String,
    pub compatible: Vec<String>,
}

impl SocInfo {
    pub fn from_device_tree(fdt: &FdtParser) -> Result<Self> {
        let model = fdt.read_property("/", "model")
            .and_then(|data| parse_string(data).ok())
            .map(|s| String::from(s))
            .unwrap_or_else(|| String::from("Unknown"));

        let compatible = fdt.read_property("/", "compatible")
            .and_then(|data| parse_stringlist(data).ok())
            .map(|list| list.iter().map(|s| String::from(*s)).collect::<Vec<String>>())
            .unwrap_or_else(|| Vec::new());

        let family = Self::detect_soc_family(&model, &compatible);

        Ok(Self {
            family,
            model,
            compatible,
        })
    }

    fn detect_soc_family(model: &str, compatible: &[String]) -> SocFamily {
        let model_lower = model.to_lowercase();
        let compat_str = compatible.join(" ").to_lowercase();
        let combined = format!("{} {}", model_lower, compat_str);

        if combined.contains("qualcomm") || combined.contains("qcom") {
            return SocFamily::Qualcomm(Self::detect_snapdragon(&combined));
        }

        if combined.contains("mediatek") || combined.contains("mt") {
            return SocFamily::MediaTek(Self::detect_dimensity(&combined));
        }

        if combined.contains("exynos") || combined.contains("samsung") {
            return SocFamily::Exynos(Self::detect_exynos(&combined));
        }

        SocFamily::Unknown
    }

    fn detect_snapdragon(text: &str) -> SnapdragonModel {
        if text.contains("845") || text.contains("sdm845") {
            SnapdragonModel::SD845
        } else if text.contains("855") || text.contains("sm8150") {
            SnapdragonModel::SD855
        } else if text.contains("865") || text.contains("sm8250") {
            SnapdragonModel::SD865
        } else if text.contains("888") || text.contains("sm8350") {
            SnapdragonModel::SD888
        } else if text.contains("8 gen 1") || text.contains("sm8450") {
            SnapdragonModel::SD8Gen1
        } else if text.contains("8 gen 2") || text.contains("sm8550") {
            SnapdragonModel::SD8Gen2
        } else if text.contains("660") || text.contains("sdm660") {
            SnapdragonModel::SD660
        } else if text.contains("665") || text.contains("sm6125") {
            SnapdragonModel::SD665
        } else if text.contains("675") || text.contains("sm6150") {
            SnapdragonModel::SD675
        } else if text.contains("710") || text.contains("sdm710") {
            SnapdragonModel::SD710
        } else if text.contains("720g") || text.contains("sm7125") {
            SnapdragonModel::SD720G
        } else if text.contains("730") || text.contains("sm7150") {
            SnapdragonModel::SD730
        } else if text.contains("750g") || text.contains("sm7225") {
            SnapdragonModel::SD750G
        } else if text.contains("765g") || text.contains("sm7250") {
            SnapdragonModel::SD765G
        } else if text.contains("778g") || text.contains("sm7325") {
            SnapdragonModel::SD778G
        } else if text.contains("780g") || text.contains("sm7350") {
            SnapdragonModel::SD780G
        } else if text.contains("695") || text.contains("sm6375") {
            SnapdragonModel::SD695
        } else if text.contains("480") || text.contains("sm4350") {
            SnapdragonModel::SD480
        } else if text.contains("460") || text.contains("sm4250") {
            SnapdragonModel::SD460
        } else if text.contains("439") || text.contains("sdm439") {
            SnapdragonModel::SD439
        } else if text.contains("429") || text.contains("sdm429") {
            SnapdragonModel::SD429
        } else {
            SnapdragonModel::Unknown(String::from(text))
        }
    }

    fn detect_dimensity(text: &str) -> DimensityModel {
        if text.contains("700") || text.contains("mt6833") {
            DimensityModel::D700
        } else if text.contains("720") || text.contains("mt6853") {
            DimensityModel::D720
        } else if text.contains("800") || text.contains("mt6873") {
            DimensityModel::D800
        } else if text.contains("810") {
            DimensityModel::D810
        } else if text.contains("820") || text.contains("mt6875") {
            DimensityModel::D820
        } else if text.contains("900") || text.contains("mt6877") {
            DimensityModel::D900
        } else if text.contains("920") {
            DimensityModel::D920
        } else if text.contains("1000") || text.contains("mt6889") {
            DimensityModel::D1000
        } else if text.contains("1100") {
            DimensityModel::D1100
        } else if text.contains("1200") || text.contains("mt6893") {
            DimensityModel::D1200
        } else if text.contains("8000") {
            DimensityModel::D8000
        } else if text.contains("8100") || text.contains("mt6895") {
            DimensityModel::D8100
        } else if text.contains("8200") {
            DimensityModel::D8200
        } else if text.contains("9000") || text.contains("mt6983") {
            DimensityModel::D9000
        } else if text.contains("9200") {
            DimensityModel::D9200
        } else {
            DimensityModel::Unknown(String::from(text))
        }
    }

    fn detect_exynos(text: &str) -> ExynosModel {
        if text.contains("850") {
            ExynosModel::E850
        } else if text.contains("880") {
            ExynosModel::E880
        } else if text.contains("980") {
            ExynosModel::E980
        } else if text.contains("990") {
            ExynosModel::E990
        } else if text.contains("1080") {
            ExynosModel::E1080
        } else if text.contains("1280") {
            ExynosModel::E1280
        } else if text.contains("1330") {
            ExynosModel::E1330
        } else if text.contains("1380") {
            ExynosModel::E1380
        } else if text.contains("2100") {
            ExynosModel::E2100
        } else if text.contains("2200") {
            ExynosModel::E2200
        } else {
            ExynosModel::Unknown(String::from(text))
        }
    }

    pub fn is_qualcomm(&self) -> bool {
        matches!(self.family, SocFamily::Qualcomm(_))
    }

    pub fn is_mediatek(&self) -> bool {
        matches!(self.family, SocFamily::MediaTek(_))
    }

    pub fn is_exynos(&self) -> bool {
        matches!(self.family, SocFamily::Exynos(_))
    }

    pub fn get_name(&self) -> &str {
        &self.model
    }

    pub fn get_compatible_strings(&self) -> &[String] {
        &self.compatible
    }
}

pub fn detect_soc(fdt: &FdtParser) -> SocFamily {
    SocInfo::from_device_tree(fdt)
        .map(|info| info.family)
        .unwrap_or(SocFamily::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_snapdragon_845() {
        let model = SocInfo::detect_snapdragon("qualcomm sdm845");
        assert_eq!(model, SnapdragonModel::SD845);
    }

    #[test]
    fn test_detect_snapdragon_888() {
        let model = SocInfo::detect_snapdragon("qualcomm sm8350");
        assert_eq!(model, SnapdragonModel::SD888);
    }

    #[test]
    fn test_detect_dimensity_1200() {
        let model = SocInfo::detect_dimensity("mediatek mt6893");
        assert_eq!(model, DimensityModel::D1200);
    }

    #[test]
    fn test_detect_exynos_2100() {
        let model = SocInfo::detect_exynos("samsung exynos 2100");
        assert_eq!(model, ExynosModel::E2100);
    }

    #[test]
    fn test_soc_family_equality() {
        let soc1 = SocFamily::Qualcomm(SnapdragonModel::SD845);
        let soc2 = SocFamily::Qualcomm(SnapdragonModel::SD845);
        assert_eq!(soc1, soc2);
    }

    #[test]
    fn test_soc_info_qualcomm() {
        let info = SocInfo {
            family: SocFamily::Qualcomm(SnapdragonModel::SD845),
            model: String::from("Snapdragon 845"),
            compatible: vec![String::from("qcom,sdm845")],
        };
        assert!(info.is_qualcomm());
        assert!(!info.is_mediatek());
        assert!(!info.is_exynos());
    }

    #[test]
    fn test_soc_info_mediatek() {
        let info = SocInfo {
            family: SocFamily::MediaTek(DimensityModel::D1200),
            model: String::from("Dimensity 1200"),
            compatible: vec![String::from("mediatek,mt6893")],
        };
        assert!(!info.is_qualcomm());
        assert!(info.is_mediatek());
        assert!(!info.is_exynos());
    }

    #[test]
    fn test_detect_sd660() {
        let model = SocInfo::detect_snapdragon("qualcomm sdm660");
        assert_eq!(model, SnapdragonModel::SD660);
    }

    #[test]
    fn test_detect_sd8gen2() {
        let model = SocInfo::detect_snapdragon("qualcomm sm8550 8 gen 2");
        assert_eq!(model, SnapdragonModel::SD8Gen2);
    }
}
