#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Provider {
    pub name: &'static str,
    pub percent: u8,
    pub used: &'static str,
    pub limit: &'static str,
    pub color: [u8; 3],
}

pub const PROVIDERS: [Provider; 5] = [
    Provider {
        name: "Cursor",
        percent: 42,
        used: "$21",
        limit: "$50",
        color: [126, 108, 255],
    },
    Provider {
        name: "Copilot",
        percent: 67,
        used: "2,010",
        limit: "3,000",
        color: [82, 180, 255],
    },
    Provider {
        name: "Claude",
        percent: 27,
        used: "$13.50",
        limit: "$50",
        color: [255, 162, 93],
    },
    Provider {
        name: "Codex",
        percent: 52,
        used: "26k",
        limit: "50k",
        color: [103, 213, 163],
    },
    Provider {
        name: "OpenCode",
        percent: 38,
        used: "$7.60",
        limit: "$20",
        color: [239, 116, 160],
    },
];

pub const DAYS: [u8; 12] = [28, 45, 34, 58, 40, 72, 51, 65, 43, 82, 61, 74];

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}
