pub fn normalize_for_wer(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars() {
        let keep = c.is_ascii_alphanumeric() || c == '\'';
        if keep {
            out.push(c.to_ascii_lowercase());
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

pub fn tokenize_words(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split_whitespace().collect()
}

#[derive(Debug, Clone, Copy)]
pub struct WerStats {
    pub edits: usize,
    pub ref_words: usize,
    pub wer: f32,
}

pub fn wer(ref_text: &str, hyp_text: &str) -> WerStats {
    let r = normalize_for_wer(ref_text);
    let h = normalize_for_wer(hyp_text);
    let r_tok = tokenize_words(&r);
    let h_tok = tokenize_words(&h);
    let edits = levenshtein_tokens(&r_tok, &h_tok);
    let ref_words = r_tok.len();
    let wer = if ref_words == 0 {
        if h_tok.is_empty() { 0.0 } else { 1.0 }
    } else {
        edits as f32 / ref_words as f32
    };
    WerStats {
        edits,
        ref_words,
        wer,
    }
}

fn levenshtein_tokens(a: &[&str], b: &[&str]) -> usize {
    // DP with O(min(m,n)) memory.
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let m = short.len();
    let n = long.len();
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];

    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if long[i - 1] == short[j - 1] { 0 } else { 1 };
            let del = prev[j] + 1;
            let ins = curr[j - 1] + 1;
            let sub = prev[j - 1] + cost;
            curr[j] = del.min(ins).min(sub);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(normalize_for_wer("Hello, World!"), "hello world");
        assert_eq!(normalize_for_wer("it's  OK."), "it's ok");
    }

    #[test]
    fn test_wer_exact() {
        let s = wer("A B C", "a b c");
        assert_eq!(s.edits, 0);
        assert_eq!(s.ref_words, 3);
        assert_eq!(s.wer, 0.0);
    }

    #[test]
    fn test_wer_simple() {
        // ref: a b c d
        // hyp: a b x d  => 1 substitution
        let s = wer("a b c d", "a b x d");
        assert_eq!(s.edits, 1);
        assert_eq!(s.ref_words, 4);
        assert!((s.wer - 0.25).abs() < 1e-6);
    }
}


