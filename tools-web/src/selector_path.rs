use scraper::element_ref::ElementRef;

pub struct CssPath {
    pub full: String,
    pub short: String,
}

pub fn build_paths(element: ElementRef) -> CssPath {
    let mut full_parts: Vec<String> = Vec::new();
    let mut short_parts: Vec<String> = Vec::new();

    for ancestor in element.ancestors() {
        let Some(elem) = ElementRef::wrap(ancestor) else {
            continue;
        };
        let part = element_part(&elem);

        let has_id = elem.value().id().is_some();
        let has_class = elem.value().classes().next().is_some();
        let is_semantic = matches!(
            elem.value().name(),
            "article" | "main" | "nav" | "header" | "footer" | "section" | "aside"
        );

        if has_id || has_class || is_semantic {
            short_parts.push(part.clone());
        }

        full_parts.push(part);
    }

    full_parts.reverse();
    short_parts.reverse();

    if short_parts.is_empty() && !full_parts.is_empty() {
        short_parts.push(full_parts.last().unwrap().clone());
    }

    CssPath {
        full: full_parts.join(" > "),
        short: short_parts.join(" "),
    }
}

fn element_part(element: &ElementRef) -> String {
    let tag = element.value().name();
    if let Some(id) = element.value().id() {
        return format!("{tag}#{id}");
    }
    let mut classes: Vec<&str> = element.value().classes().collect();
    if !classes.is_empty() {
        classes.sort();
        return format!("{tag}.{}", classes.join("."));
    }
    let nth = nth_child_of_type(element);
    format!("{tag}:nth-child({nth})")
}

fn nth_child_of_type(element: &ElementRef) -> usize {
    let tag = element.value().name();
    let mut count = 1;
    let mut current = element.prev_sibling();
    while let Some(sibling) = current {
        if let Some(sibling_elem) = ElementRef::wrap(sibling) {
            if sibling_elem.value().name() == tag {
                count += 1;
            }
            current = sibling_elem.prev_sibling();
        } else {
            break;
        }
    }
    count
}
