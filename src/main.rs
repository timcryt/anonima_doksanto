use once_cell::sync::OnceCell;
use quick_xml::{events::Event, Reader};
use rand::{seq::SliceRandom, thread_rng};
use regex::Regex;
use std::{collections::HashMap, io::prelude::*};

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_derive;

#[derive(Deserialize)]
struct Config {
    pub min_w: usize,
    pub min_n: usize,
    pub start_page: usize,
    pub to_lower: bool,
    pub mult: f64,
    pub base_prob: f64,
}

static CONFIG: OnceCell<Config> = OnceCell::new();

fn in_blacklist(author: &str) -> bool {
    lazy_static! {
        static ref R1: Regex = Regex::new(r".+ via @.+").unwrap();
        static ref R2: Regex = Regex::new(r"\d\d.\d\d.\d\d\d\d \d\d:\d\d:\d\d").unwrap();
    }

    match author {
        "Deleted Account"
        | ""
        | "Anonymous Telegram Bot"
        | "Anonima Roboto"
        | "Robotino"
        | "Anonymous telegram bot" => true,
        x if R1.is_match(x) => true,
        x if R2.is_match(x) => true,
        _ => false,
    }
}

fn lemmatize(data: String) -> String {
    lazy_static! {
        static ref R1: Regex = Regex::new(r"\shttps?://\S+").unwrap();
        static ref R2: Regex = Regex::new(r"\s+").unwrap();
    }

    let data = R1.replace_all(&(" ".to_string() + &data), " ").into_owned();
    let data = R2.replace_all(&data, " ").into_owned();

    let data = if CONFIG.get().unwrap().to_lower {
        data.to_lowercase()
    } else {
        data
    };

    data.trim().to_string()
}

fn space_count(data: &str) -> usize {
    data.chars().filter(|c| *c == ' ').count()
}

fn parse(docs_str: String) -> (Vec<(usize, String)>, HashMap<String, usize>, Vec<String>) {
    let mut docs = Vec::new();
    let mut authorlist: HashMap<String, _> = HashMap::new();
    let mut revlist = vec![String::new()];

    let mut reader = Reader::from_str(&docs_str);
    reader.trim_text(true);
    reader.check_end_names(false);
    let mut div_class = vec![String::new()];
    let mut author = String::new();
    let mut authortemp: HashMap<String, _> = HashMap::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name() {
                b"div" => {
                    for attr in e.attributes().map(|a| a.unwrap()) {
                        if attr.key == b"class" {
                            div_class.push(String::from_utf8(attr.value.into_owned()).unwrap());
                        }
                    }
                }
                _ => (),
            },
            Ok(Event::End(ref e)) => match e.name() {
                b"div" => {
                    div_class.pop();
                }
                _ => (),
            },
            Ok(Event::Text(e)) => {
                if div_class[div_class.len() - 1] == "text" && !in_blacklist(&author) {
                    let data = lemmatize(e.unescape_and_decode(&reader).unwrap());
                    if space_count(&data) >= CONFIG.get().unwrap().min_w - 1 {
                        if authortemp.get(&author).is_none() {
                            authortemp.insert(author.clone(), Some(Vec::new()));
                        } else if authortemp[&author].is_none() {
                            docs.push((authorlist[&author], data));
                        } else {
                            authortemp
                                .get_mut(&author)
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .push(data);
                            if authortemp[&author].as_ref().unwrap().len()
                                == CONFIG.get().unwrap().min_n
                            {
                                authorlist.insert(author.clone(), authorlist.len() + 1);
                                revlist.push(author.clone());
                                for msg in authortemp.get_mut(&author).unwrap().take().unwrap() {
                                    docs.push((authorlist[&author], msg));
                                }
                            }
                        }
                    }
                } else if div_class[div_class.len() - 1] == "from_name" {
                    let data = e.unescape_and_decode(&reader).unwrap();
                    author = data.trim().to_string();
                }
            }
            //Err(e) => panic!("Error at position {}: {:?}", reader.buffer_position(), e),
            Ok(Event::Eof) => break,
            _ => (),
        }
    }
    buf.clear();
    (docs, authorlist, revlist)
}

fn divide<T>(mut docs: Vec<T>, test_size: f64) -> (Vec<T>, Vec<T>) {
    assert!(test_size >= 0.0 && test_size <= 1.0);
    let mut rng = thread_rng();
    docs.shuffle(&mut rng);
    let mut test_suit = Vec::new();
    let (mut i, n) = (0, docs.len());

    while test_size != 0.0 && i as f64 / test_size < n as f64 {
        test_suit.push(docs.pop().unwrap());
        i += 1;
    }

    (docs, test_suit)
}

fn learn_markov(docs: Vec<(usize, String)>) -> Vec<HashMap<String, HashMap<String, f64>>> {
    let n = docs.iter().map(|x| x.0).max().unwrap();
    let mut res = vec![HashMap::new(); n + 1];

    for (author, doc) in docs {
        let chain = res.get_mut(author).unwrap();
        let mut oc = String::new();
        for c in doc.chars() {
            *chain
                .entry(oc.clone())
                .or_insert(HashMap::new())
                .entry(c.to_string())
                .or_insert(0.0) += 1.0;
            oc = c.to_string();
        }
        *chain
            .entry(oc.clone())
            .or_insert(HashMap::new())
            .entry("".to_string())
            .or_insert(0.0) += 1.0;
    }

    for chain in &mut res {
        for (_, ending) in chain {
            let s: f64 = ending.values().sum();
            for (_, v) in ending {
                *v /= s;
            }
        }
    }

    res
}

fn predict(chains: &Vec<HashMap<String, HashMap<String, f64>>>, doc: &str) -> Vec<(f64, usize)> {
    let mut probs = Vec::new();
    for (i, chain) in chains.iter().enumerate().skip(1) {
        let mut prob = 1.0;
        let mut oc = String::new();
        for c in doc.chars() {
            prob *= chain
                .get(&oc)
                .and_then(|e| e.get(&c.to_string()).copied())
                .unwrap_or(CONFIG.get().unwrap().base_prob);

            prob *= CONFIG.get().unwrap().mult;
            oc = c.to_string();
        }

        probs.push((prob, i));
    }

    let s: f64 = probs.iter().map(|(p, _)| p).sum();
    for prob in &mut probs {
        prob.0 /= s;
    }
    probs.sort_by(|a, b| {
        std::cmp::PartialOrd::partial_cmp(&b.0, &a.0).unwrap_or(std::cmp::Ordering::Equal)
    });

    probs
}

fn test(
    chains: &Vec<HashMap<String, HashMap<String, f64>>>,
    test_suit: Vec<(usize, String)>,
) -> f64 {
    test_suit
        .iter()
        .filter_map(|(auth, doc)| {
            if *auth == predict(chains, doc)[0].1 {
                Some(())
            } else {
                None
            }
        })
        .count() as f64
        / test_suit.len() as f64
}

fn read_docs() -> Option<String> {
    let mut docs_str = String::new();

    let arch = std::fs::File::open("babilejo.zip").ok()?;
    let mut arch = zip::ZipArchive::new(arch).ok()?;

    let mut buf = String::new();

    for i in (CONFIG.get().unwrap().start_page)..=176 {
        let s = format!(
            "messages{}.html",
            if i == 1 {
                "".to_string()
            } else {
                i.to_string()
            }
        );

        let mut f = arch.by_name(&s).ok()?;
        buf.clear();
        f.read_to_string(&mut buf).ok()?;

        docs_str += &buf;
    }

    Some(docs_str)
}

fn main() {
    println!("Konfiguracio alŝultitiĝas...");

    CONFIG
        .set(
            if let Some(s) = std::fs::read_to_string("conf.txt")
                .ok()
                .and_then(|s| ron::from_str(&s).ok())
            {
                s
            } else {
                eprintln!("Eraro: dosiero conf.txt ne ekzistas aŭ estas rompita");
                return;
            },
        )
        .ok()
        .unwrap();

    eprintln!("Datumbazo alŝutitiĝas...");

    let docs_str = if let Some(s) = read_docs() {
        s
    } else {
        eprintln!("Eraro: dosiero babilejo.zip ne ekzistas aŭ estas rompita");
        return;
    };

    eprintln!("Datumbazo traktitiĝas...");
    let (doc, _authors, authors_rev) = parse(docs_str);
    let (doc_learn, doc_test) = divide(doc, 0.2);

    eprintln!("Markov-ĉeno kreitiĝas...");
    let markov = learn_markov(doc_learn);

    eprintln!("Markov-ĉeno ekzamenitiĝas..");
    let mark = test(&markov, doc_test);
    eprintln!("Precizeco: {:.2}%", mark * 100.0);

    let msg_str = std::fs::read_to_string("msg.txt").unwrap_or(String::new());

    if msg_str.trim().is_empty() {
        eprintln!("Eraro: dosiero \"msg.txt\" ne ekzistas aŭ malplenas");
        return;
    }

    let authors = predict(&markov, &lemmatize(msg_str));

    for (p, a) in authors {
        println!("{:9.4} {}", p.ln(), authors_rev[a]);
    }
}
