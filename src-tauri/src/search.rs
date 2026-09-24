//! 搜索索引与排序。
//!
//! 排序公式遵循 PRD 6.3：
//! ```text
//! score = 收藏权重 + 使用频率 * 10 + 最近使用衰减 + 模糊匹配得分
//! ```
//! 其中「匹配得分」按命中质量分档（精确 > 前缀 > 词首 > 子串 > 拼音 > 模糊），
//! 量级压在 0~1000；收藏 1000、置顶 1500、频率最高 500、最近使用最高 300。
//! 这样「匹配质量」与「使用习惯」不会互相淹没：同等匹配下收藏必然胜出，
//! 但一个精确命中的生僻应用仍能压过一个模糊命中的常用应用。

use pinyin::ToPinyin;

use crate::config::Config;
use crate::model::{LauncherItem, SearchHit};
use crate::state::{now_secs, Usage};

// 命中质量分档
const TIER_EXACT: i64 = 1000;
const TIER_PREFIX: i64 = 900;
const TIER_WORD: i64 = 820;
const TIER_SUBSTR: i64 = 720;
/// 别名（拼音全拼 / 去分隔符拼写）完全相等
const TIER_ALIAS_EXACT: i64 = 700;
const TIER_INITIALS_PREFIX: i64 = 660;
const TIER_ALIAS_PREFIX: i64 = 630;
const TIER_INITIALS_SUBSTR: i64 = 560;
const TIER_ALIAS_SUBSTR: i64 = 500;
const TIER_KEYWORD: i64 = 450;
const TIER_FUZZY: i64 = 380;
const TIER_KEYWORD_SUBSTR: i64 = 330;
const TIER_TARGET: i64 = 240;

/// 低于此分值视为「没搜到」，避免模糊匹配把整个列表填满噪声。
const MIN_SCORE: i64 = 200;

const BONUS_PINNED: i64 = 1500;
const BONUS_FAVORITE: i64 = 1000;
const FREQ_UNIT: i64 = 10;
const FREQ_CAP: u32 = 50;
const RECENCY_MAX: f64 = 300.0;
/// 最近使用权重的半衰期（小时）：一周前的使用记录只值一半分。
const RECENCY_HALF_LIFE_HOURS: f64 = 168.0;

/// 预计算索引项。搜索热路径上只做字符串比较，不做任何分配。
pub struct IndexedItem {
    pub item: LauncherItem,
    pub name_lower: String,
    /// 剔除分隔符的小写形式，让 `vscode` 命中 `VS Code`
    pub compact: String,
    /// 可搜索别名：汉字转拼音全拼（`微信` → `weixin`），
    /// ASCII 则按分隔符与驼峰切词后拼接（`Visual Studio Code` → `visualstudiocode`）。
    /// 注意不能复用 `compact`：`char::is_alphanumeric()` 对汉字返回 true，
    /// 那样 `微信` 的 compact 仍是 `微信`，全拼检索就永远命中不了。
    pub alias: String,
    /// 首字母串：`微信` → `wx`，`Visual Studio Code` → `vsc`
    pub initials: String,
    pub target_lower: String,
    pub keywords_lower: Vec<String>,
    pub words: Vec<String>,
}

impl IndexedItem {
    /// 参与「别名」类匹配的两个候选串。
    fn alias_forms(&self) -> [&str; 2] {
        [self.compact.as_str(), self.alias.as_str()]
    }
}

pub fn build_index(items: Vec<LauncherItem>) -> Vec<IndexedItem> {
    items
        .into_iter()
        .map(|item| {
            let (alias, initials) = phonetics(&item.name);
            let compact = compact_of(&item.name);
            let words = item
                .name
                .to_lowercase()
                .split_whitespace()
                .map(|w| w.to_string())
                .collect();
            IndexedItem {
                name_lower: item.name.to_lowercase(),
                compact: if compact.is_empty() {
                    item.id.to_lowercase()
                } else {
                    compact
                },
                alias,
                initials: if initials.is_empty() {
                    compact_of(&item.name)
                } else {
                    initials
                },
                target_lower: item
                    .target
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase(),
                keywords_lower: item.keywords.iter().map(|k| k.to_lowercase()).collect(),
                words,
                item,
            }
        })
        .collect()
}

/// 主查询入口。
pub fn search(
    index: &[IndexedItem],
    config: &Config,
    usage: &Usage,
    query: &str,
    limit: usize,
) -> Vec<SearchHit> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return empty_state(index, config, usage, limit);
    }

    let tokens: Vec<String> = trimmed
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    let token_count = tokens.len() as i64;

    // 先收集 (得分, 索引下标, 高亮)，排序截断后再克隆条目，
    // 避免为一个被丢掉的结果白克隆一次 LauncherItem。
    let mut scored: Vec<(i64, usize, Vec<[u32; 2]>)> = Vec::new();
    for (position, entry) in index.iter().enumerate() {
        let mut sum = 0i64;
        let mut highlights: Vec<[u32; 2]> = Vec::new();
        let mut all_matched = true;

        for token in &tokens {
            match match_token(entry, token) {
                Some((score, hl)) => {
                    sum += score;
                    if highlights.is_empty() {
                        highlights = hl;
                    }
                }
                None => {
                    all_matched = false;
                    break;
                }
            }
        }
        if !all_matched {
            continue;
        }

        let base = sum / token_count;
        if base < MIN_SCORE {
            continue;
        }
        scored.push((base + bonuses(config, usage, &entry.item.id), position, highlights));
    }

    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| index[a.1].name_lower.cmp(&index[b.1].name_lower))
    });
    scored.truncate(limit);
    scored
        .into_iter()
        .map(|(score, position, highlights)| SearchHit {
            item: index[position].item.clone(),
            score,
            highlights,
        })
        .collect()
}

/// 空查询：只给「置顶 → 收藏 → 最近使用」，不给全量应用列表。
/// PRD F-10 的要求是「不显示空白」而不是「显示全部」，混入随机应用只会干扰。
fn empty_state(
    index: &[IndexedItem],
    config: &Config,
    usage: &Usage,
    limit: usize,
) -> Vec<SearchHit> {
    let mut seeded: Vec<(u8, i64, usize)> = Vec::new();
    for (position, entry) in index.iter().enumerate() {
        let id = &entry.item.id;
        let last_used = usage.get(id).map(|u| u.last_used_at).unwrap_or(0);
        let tier = if config.pinned.iter().any(|x| x == id) {
            0
        } else if config.favorites.iter().any(|x| x == id) {
            1
        } else if last_used > 0 {
            2
        } else {
            continue;
        };
        seeded.push((tier, last_used as i64, position));
    }
    // 层级优先，其次越近使用越靠前
    seeded.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
    seeded.truncate(limit);

    seeded
        .into_iter()
        .map(|(tier, last_used, position)| {
            let tier_bonus = match tier {
                0 => BONUS_PINNED,
                1 => BONUS_FAVORITE,
                _ => 0,
            };
            let score = tier_bonus + recency_bonus(last_used.max(0) as u64);
            SearchHit {
                item: index[position].item.clone(),
                score,
                highlights: Vec::new(),
            }
        })
        .collect()
}

fn bonuses(config: &Config, usage: &Usage, id: &str) -> i64 {
    let mut score = 0i64;
    if config.pinned.iter().any(|x| x == id) {
        score += BONUS_PINNED;
    } else if config.favorites.iter().any(|x| x == id) {
        score += BONUS_FAVORITE;
    }
    if let Some(entry) = usage.get(id) {
        score += entry.count.min(FREQ_CAP) as i64 * FREQ_UNIT;
        score += recency_bonus(entry.last_used_at);
    }
    score
}

fn recency_bonus(last_used_at: u64) -> i64 {
    if last_used_at == 0 {
        return 0;
    }
    let age = now_secs().saturating_sub(last_used_at) as f64;
    let hours = age / 3600.0;
    (RECENCY_MAX * 0.5f64.powf(hours / RECENCY_HALF_LIFE_HOURS)).round() as i64
}

/// 单个关键词令牌对单个条目的最佳匹配档位与高亮区间。
fn match_token(entry: &IndexedItem, token: &str) -> Option<(i64, Vec<[u32; 2]>)> {
    let name = entry.name_lower.as_str();
    let name_chars = name.chars().count() as u32;

    if name == token {
        return Some((TIER_EXACT, vec![[0, name_chars]]));
    }
    if name.starts_with(token) {
        return Some((TIER_PREFIX, vec![[0, token.chars().count() as u32]]));
    }

    // 词首命中：`code` 命中 `Visual Studio Code`
    let mut from = 0usize;
    while let Some(relative) = name[from..].find(token) {
        let position = from + relative;
        let at_word_start = position == 0
            || !name[..position]
                .chars()
                .next_back()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false);
        if at_word_start {
            return Some((TIER_WORD, highlight(name, position, token)));
        }
        from = position + token.len();
    }

    if let Some(position) = name.find(token) {
        let penalty = char_offset(name, position).min(60) as i64;
        return Some((TIER_SUBSTR - penalty, highlight(name, position, token)));
    }

    // 以下档位都基于「去掉分隔符」或「拼音」形态，因此不再能映射回原名做高亮。
    let compact_token = compact_of(token);
    if compact_token.is_empty() {
        return None;
    }

    let forms = entry.alias_forms();
    if forms.iter().any(|form| *form == compact_token.as_str()) {
        return Some((TIER_ALIAS_EXACT, Vec::new()));
    }
    if entry.initials.starts_with(compact_token.as_str()) {
        return Some((TIER_INITIALS_PREFIX, Vec::new()));
    }
    if forms.iter().any(|form| form.starts_with(compact_token.as_str())) {
        return Some((TIER_ALIAS_PREFIX, Vec::new()));
    }
    if entry.initials.contains(compact_token.as_str()) {
        return Some((TIER_INITIALS_SUBSTR, Vec::new()));
    }
    if forms.iter().any(|form| form.contains(compact_token.as_str())) {
        let earliest = forms
            .iter()
            .filter_map(|form| form.find(compact_token.as_str()))
            .min()
            .unwrap_or(0);
        return Some((TIER_ALIAS_SUBSTR - earliest.min(80) as i64, Vec::new()));
    }
    if entry
        .keywords_lower
        .iter()
        .any(|k| k == compact_token.as_str() || k.starts_with(compact_token.as_str()))
    {
        return Some((TIER_KEYWORD, Vec::new()));
    }
    // 模糊子序列：两种形态取更贴合的那个
    let fuzzy = forms
        .iter()
        .filter_map(|form| subsequence_quality(form, compact_token.as_str()))
        .fold(0.0f64, f64::max);
    if fuzzy > 0.0 {
        return Some((
            (TIER_FUZZY as f64 * (0.55 + 0.45 * fuzzy)) as i64,
            Vec::new(),
        ));
    }
    if entry.keywords_lower.iter().any(|k| k.contains(compact_token.as_str())) {
        return Some((TIER_KEYWORD_SUBSTR, Vec::new()));
    }
    if !entry.target_lower.is_empty() && entry.target_lower.contains(token) {
        return Some((TIER_TARGET, Vec::new()));
    }
    None
}

fn highlight(name: &str, byte_position: usize, token: &str) -> Vec<[u32; 2]> {
    vec![[
        char_offset(name, byte_position),
        token.chars().count() as u32,
    ]]
}

fn char_offset(text: &str, byte_position: usize) -> u32 {
    if byte_position > text.len() {
        return 0;
    }
    text[..byte_position].chars().count() as u32
}

fn compact_of(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 子序列匹配质量：`chrme` → `chrome`。
/// 返回 (0,1] 的贴合度，越紧凑（间隔越少）越接近 1。
fn subsequence_quality(haystack: &str, needle: &str) -> Option<f64> {
    let hay: Vec<char> = haystack.chars().collect();
    let want: Vec<char> = needle.chars().collect();
    if want.is_empty() || hay.is_empty() || want.len() > hay.len() {
        return None;
    }
    let mut matched = 0usize;
    let mut consumed = 0usize;
    while consumed < hay.len() && matched < want.len() {
        if hay[consumed] == want[matched] {
            matched += 1;
        }
        consumed += 1;
    }
    if matched < want.len() {
        return None;
    }
    let quality = want.len() as f64 / consumed.max(1) as f64;
    Some(quality.clamp(0.0, 1.0))
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F
    )
}

/// 生成 (全拼, 首字母)。
///
/// 汉字走拼音：`微信` → (`weixin`, `wx`)。
/// ASCII 按分隔符 *和驼峰* 切词：`Visual Studio Code` → `vsc`，
/// `VisualStudioCode` 同样得到 `vsc`。
fn phonetics(name: &str) -> (String, String) {
    fn flush_cjk(buf: &mut String, full: &mut String, initials: &mut String) {
        if buf.is_empty() {
            return;
        }
        for syllable in buf.as_str().to_pinyin().flatten() {
            full.push_str(syllable.plain());
            initials.push(syllable.first_letter());
        }
        buf.clear();
    }
    fn flush_ascii(buf: &mut String, full: &mut String, initials: &mut String) {
        if buf.is_empty() {
            return;
        }
        full.push_str(&buf.to_lowercase());
        if let Some(first) = buf.chars().next() {
            initials.push(first.to_ascii_lowercase());
        }
        buf.clear();
    }

    let mut full = String::new();
    let mut initials = String::new();
    let mut cjk_buf = String::new();
    let mut ascii_buf = String::new();

    for ch in name.chars() {
        if is_cjk(ch) {
            flush_ascii(&mut ascii_buf, &mut full, &mut initials);
            cjk_buf.push(ch);
        } else if ch.is_alphanumeric() {
            flush_cjk(&mut cjk_buf, &mut full, &mut initials);
            // 驼峰切词：已经是第二个词了
            if ch.is_uppercase() && !ascii_buf.is_empty() {
                flush_ascii(&mut ascii_buf, &mut full, &mut initials);
            }
            ascii_buf.push(ch);
        } else {
            flush_cjk(&mut cjk_buf, &mut full, &mut initials);
            flush_ascii(&mut ascii_buf, &mut full, &mut initials);
        }
    }
    flush_cjk(&mut cjk_buf, &mut full, &mut initials);
    flush_ascii(&mut ascii_buf, &mut full, &mut initials);

    (full, initials)
}
