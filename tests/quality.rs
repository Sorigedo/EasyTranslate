use easytranslate::quality::number_warning;
#[test]
fn warns_if_payment_amount_changes() {
    assert!(
        number_warning(
            "支付50美元，不是500美元",
            "Pay 500 dollars, not 500 dollars"
        )
        .is_some()
    );
}
#[test]
fn number_order_can_change_without_warning() {
    assert!(number_warning("3人分100美元", "Divide 100 dollars among 3 people").is_none());
}
#[test]
fn thousands_separators_and_trailing_sentence_punctuation_are_not_changes() {
    assert!(number_warning("金额1,500.25美元", "The amount is 1500.25.").is_none());
}
#[test]
fn does_not_claim_semantic_correctness_when_numbers_match() {
    assert!(number_warning("请不要支付50美元", "Please pay 50 dollars").is_none());
}
