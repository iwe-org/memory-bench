You are enriching cross-references in an encyclopedia corpus. Below is one article page from the corpus (markdown; existing links look like [text](key)) and a list of candidate related articles from the same corpus.

Identify mentions in the page that refer to one of the candidate articles but are not already linked. A mention may be abbreviated, partial, descriptive, or phrased differently than the candidate's title — a person's surname, a work referred to as "the novel of the same name", a former or translated name. Only propose a link when the page makes it clear the mention refers to that specific candidate; do not link on topic similarity alone, and do not link a candidate that is merely about a related subject.

Return a JSON array, nothing else. Each entry: {"text": "<exact substring of the page to turn into a link>", "key": "<candidate key>"}. The text must appear verbatim in the page outside existing links. Return [] if nothing qualifies.

Example from an unrelated corpus: a page saying "she adapted her debut novel into a 2024 film" with a candidate `first-light-novel` titled "First Light (novel)" whose article names her as the author would yield [{"text": "her debut novel", "key": "first-light-novel"}].

# Page: {key}

{content}

# Candidates:

{candidates}
