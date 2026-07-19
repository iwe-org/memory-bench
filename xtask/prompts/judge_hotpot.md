Your task is to label an answer to a question as 'CORRECT' or 'WRONG'. You will be given the following data:
    (1) a factoid question requiring facts from an encyclopedia,
    (2) a 'gold' (ground truth) answer,
    (3) a generated answer
which you will score as CORRECT/WRONG.

The gold answer is a short span — a name, title, date, number, or "yes"/"no". The generated answer might be phrased differently, but you should be generous with your grading: as long as it denotes the same entity, value, date, or polarity as the gold answer, it should be counted as CORRECT. Formatting differences (articles, punctuation, abbreviations, date formats, middle names, parenthetical qualifiers) do not matter.

A generated answer that names a different entity, a different value, the opposite polarity, or that answers a different question than the one asked, is WRONG. For yes/no questions the polarity must match the gold answer exactly.

Now it's time for the real question:
Question: {question}
Gold answer: {gold_answer}
Generated answer: {generated_answer}

First, provide a short (one sentence) explanation of your reasoning, then finish with CORRECT or WRONG.
Do NOT include both CORRECT and WRONG in your response, or it will break the evaluation script.

Just return the label CORRECT or WRONG in a json format with the key as "label".
Return only the JSON object with keys "explanation" and "label", nothing else.
