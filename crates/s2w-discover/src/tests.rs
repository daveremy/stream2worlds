use super::*;

/// The recorded fixture's events as the log stores them: `{"data":…,"id":…}`.
fn fixture() -> Vec<Vec<u8>> {
    let raw = include_str!("../../s2w-sources/testdata/wikipedia-page-change.raw.sse");
    let mut out = Vec::new();
    let (mut data, mut id): (Option<String>, Option<String>) = (None, None);
    for line in raw.lines() {
        if line.is_empty() {
            if let (Some(d), Some(i)) = (data.take(), id.take()) {
                out.push(serde_json::json!({"data": d, "id": i}).to_string().into_bytes());
            }
            continue;
        }
        let (k, v) = line.split_once(':').unwrap_or((line, ""));
        let v = v.strip_prefix(' ').unwrap_or(v).to_owned();
        match k {
            "data" => data = Some(data.map_or(v.clone(), |d| d + "\n" + &v)),
            "id" => id = Some(v),
            _ => {}
        }
    }
    out
}

#[test]
fn dump_fixture() {
    let events = fixture();
    let refs: Vec<&[u8]> = events.iter().map(Vec::as_slice).collect();
    let (profile, discovery) = discover(&refs, &Config::default());
    for p in &profile.paths {
        println!("{:60} n={:5} d={:5} {:?}", rule_id(&p.path), p.count, p.distinct, p.role);
    }
    println!("event_type {:?}", profile.event_type);
    match discovery {
        Discovery::Mapping(m) => println!("{}", serde_json::to_string_pretty(&m).unwrap()),
        Discovery::Abstain(r) => println!("ABSTAIN {r}"),
    }
}
