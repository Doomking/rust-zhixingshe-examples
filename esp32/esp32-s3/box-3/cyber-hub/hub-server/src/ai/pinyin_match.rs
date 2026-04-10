use pinyin::ToPinyin;
use strsim::levenshtein;
use tracing::debug;

pub struct PinyinMatcher {
    target_pinyin: String,
}

impl PinyinMatcher {
    pub fn new(target: &str) -> Self {
        let pinyin = char_to_pinyin_string(target);
        Self { target_pinyin: pinyin }
    }

    /// 检查输入的文本是否在发音上匹配目标唤醒词。
    /// 返回 (是否匹配, 匹配词之后的剩余文本)
    pub fn find_match<'a>(&self, text: &'a str, threshold: usize) -> (bool, &'a str) {
        let chars: Vec<char> = text.chars().collect();
        
        // 窗口滑动匹配：尝试匹配文本中的每一个可能的位置
        // 我们假设唤醒词在文本的前部或中部
        for i in 0..chars.len() {
            // 尝试匹配接下来的 1..5 个字符（涵盖绝大多数唤醒词长度）
            for len in 1..=5 {
                if i + len > chars.len() { break; }
                
                let slice: String = chars[i..i+len].iter().collect();
                let slice_pinyin = char_to_pinyin_string(&slice);
                
                let dist = levenshtein(&self.target_pinyin, &slice_pinyin);
                if dist <= threshold {
                    debug!("[PinyinMatch] Found potential match: \"{}\" (pinyin: {}) dist: {} vs target pinyin: {}", 
                        slice, slice_pinyin, dist, self.target_pinyin);
                    
                    // 找到匹配，返回匹配点之后的字符串
                    let byte_pos: usize = text.char_indices()
                        .nth(i + len)
                        .map(|(b, _)| b)
                        .unwrap_or(text.len());
                    
                    return (true, text[byte_pos..].trim_start_matches(|c: char| ",，。 ".contains(c)));
                }
            }
        }

        (false, "")
    }
}

/// 将汉字字符串转换为纯拼音字符串（无声调）
fn char_to_pinyin_string(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if let Some(p) = c.to_pinyin() {
            result.push_str(p.plain());
        } else {
            // 非汉字字符保留原样但转小写
            result.push_str(&c.to_lowercase().to_string());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinyin_match() {
        let matcher = PinyinMatcher::new("小光");
        
        // 精确匹配
        let (found, cmd) = matcher.find_match("小光，开启锁定", 1);
        assert!(found);
        assert_eq!(cmd, "开启锁定");

        // 音近匹配 (小广)
        let (found, _) = matcher.find_match("小广你好", 1);
        assert!(found);

        // 音近匹配 (晓光)
        let (found, _) = matcher.find_match("晓光现在几点", 1);
        assert!(found);

        // 负例
        let (found, _) = matcher.find_match("锁定屏幕", 1);
        assert!(!found);
    }
}
