use spellwake_application::{
    amount::Amount,
    policy::{PricingWindow, Rational, card_value, outgoing_allowed},
};

fn fraction(numerator: u64, denominator: u64) -> Rational {
    Rational::new(numerator.into(), denominator.into()).unwrap()
}

#[test]
fn decimal_boundary_never_accepts_numbers_or_noncanonical_text() {
    for invalid in ["", "01", "+1", "-1", "1.0", "1e3", " 1", "１"] {
        assert!(invalid.parse::<Amount>().is_err(), "accepted {invalid:?}");
    }
    assert!("9".repeat(79).parse::<Amount>().is_err());
    let large = "900719925474099300000000000000000001";
    let amount: Amount = large.parse().unwrap();
    assert_eq!(
        serde_json::to_string(&amount).unwrap(),
        format!("\"{large}\"")
    );
    assert_eq!(
        serde_json::from_str::<Amount>(&format!("\"{large}\"")).unwrap(),
        amount
    );
    assert!(serde_json::from_str::<Amount>("9007199254740993").is_err());
    assert!(Amount::from(0).checked_sub(&1.into()).is_err());
    assert!(
        "9".repeat(78)
            .parse::<Amount>()
            .unwrap()
            .checked_add(&1.into())
            .is_err()
    );
}

#[test]
fn pack_price_responds_to_quantity_modifier_and_depletion() {
    for (start, remaining, normal, modifier, expected) in [
        (100, 100, 100, fraction(1, 1), 1000),
        (100, 50, 100, fraction(1, 1), 1500),
        (50, 50, 100, fraction(1, 1), 2000),
        (100, 50, 100, fraction(2, 1), 3000),
        (100, 0, 100, fraction(1, 1), 2000),
    ] {
        let window =
            PricingWindow::new(start.into(), remaining.into(), normal.into(), modifier).unwrap();
        assert_eq!(window.valuation_quote().unwrap(), expected.into());
        if remaining == 0 {
            assert!(window.purchase_total(1).is_err());
        }
    }
}

#[test]
fn sequential_rounding_and_bulk_totals_do_not_undercharge() {
    let window = PricingWindow::new(3.into(), 3.into(), 3.into(), fraction(1, 1)).unwrap();
    // Exact successive prices are 1000, ceil(4000/3), ceil(5000/3).
    assert_eq!(window.purchase_total(3).unwrap(), 4001.into());
    assert!(window.purchase_total(0).is_err());
    assert!(window.purchase_total(4).is_err());
    for starting in 1_u64..=30 {
        let mut previous = Amount::from(0);
        for remaining in (0..=starting).rev() {
            let current =
                PricingWindow::new(starting.into(), remaining.into(), 30.into(), fraction(3, 2))
                    .unwrap()
                    .valuation_quote()
                    .unwrap();
            assert!(current >= previous);
            previous = current;
        }
    }
}

#[test]
fn invalid_pricing_sources_do_not_create_zero_or_free_quotes() {
    assert!(PricingWindow::new(0.into(), 0.into(), 1.into(), fraction(1, 1)).is_err());
    assert!(PricingWindow::new(2.into(), 3.into(), 1.into(), fraction(1, 1)).is_err());
    assert!(PricingWindow::new(2.into(), 1.into(), 0.into(), fraction(1, 1)).is_err());
    assert!(PricingWindow::new(2.into(), 1.into(), 1.into(), fraction(0, 1)).is_err());
    assert!(Rational::new(1.into(), 0.into()).is_err());
}

#[test]
fn recipe_allocation_preserves_expected_value_and_dynamic_scaling() {
    let classes = [
        (100, fraction(1, 10)),
        (80, fraction(3, 80)),
        (60, fraction(7, 480)),
        (20, fraction(1, 160)),
    ];
    let mut expected_total = Rational::whole(0.into());
    for (count, copies) in classes {
        let value = card_value(1000.into(), 260.into(), copies.clone()).unwrap();
        expected_total = expected_total.add(
            &value
                .multiply(&copies)
                .multiply(&Rational::whole(count.into())),
        );
    }
    assert_eq!(expected_total, Rational::whole(1000.into()));
    let rare = card_value(1000.into(), 260.into(), fraction(7, 480)).unwrap();
    assert_eq!(rare, fraction(24000, 91));
    let depleted = card_value(1500.into(), 260.into(), fraction(7, 480)).unwrap();
    assert_eq!(depleted, rare.multiply(&fraction(3, 2)));
}

#[test]
fn bundle_restriction_compares_rationals_without_per_card_rounding() {
    assert!(outgoing_allowed(&fraction(1300, 1), &fraction(1000, 1)));
    assert!(!outgoing_allowed(&fraction(1301, 1), &fraction(1000, 1)));
    assert!(outgoing_allowed(&fraction(0, 1), &fraction(1000, 1)));
    assert!(!outgoing_allowed(&fraction(1, 1), &fraction(0, 1)));
    // 0.34 / 0.26 exceeds 1.3; rounding both display values to zero would incorrectly pass.
    assert!(!outgoing_allowed(&fraction(34, 100), &fraction(26, 100)));
}
