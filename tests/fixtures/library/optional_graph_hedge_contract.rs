//! Independent complete optional financial controls plus actual owner/transport agreement.
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    [
        include_str!("optional-graph-hedge-resolved.json"),
        include_str!("optional-graph-hedge-direct.json"),
        include_str!("optional-graph-hedge-qualification.json"),
        include_str!("optional-graph-hedge-calibrated.json"),
    ]
    .iter()
    .flat_map(|s| {
        serde_json::from_str::<Value>(s).unwrap()["cases"]
            .as_array()
            .unwrap()
            .clone()
    })
    .filter(|c| c["id"] != "optional_graph.qualification.calibrated")
    .collect()
}
pub fn number(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
}
pub fn close(a: &Value, b: &Value, tolerance: f64) {
    let a = number(a);
    let b = number(b);
    assert!(
        a.is_finite() && b.is_finite() && (a - b).abs() <= tolerance + 1e-11 * b.abs(),
        "actual={a:.12} expected={b:.12} tolerance={tolerance:.12}"
    );
}
fn answer(case: &Value) -> Value {
    if !case["financial"].is_null() {
        return case["financial"].clone();
    }
    serde_json::from_str::<Value>(include_str!("optional-graph-hedge-reference.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == case["id"])
        .unwrap()["expected"]
        .clone()
}
pub fn request(case: &Value) -> String {
    serde_json::to_string(&case["request"]).unwrap()
}
fn price(actual: &Value, expected: &Value, input: &Value) {
    let face = number(&input["bond"]["terms"]["face_value"]);
    close(
        &actual["present_value"],
        &expected["pv"],
        0.006 * face / 100.,
    );
    for (a, b) in [
        ("dirty_price", "dirty"),
        ("clean_price", "clean"),
        ("accrued", "accrued"),
    ] {
        close(
            &actual[a],
            &expected[b],
            if a == "accrued" { 1e-10 } else { 0.006 },
        );
    }
    assert_eq!(actual["context"], input["context"]);
    assert_eq!(actual["discount_curve"], input["discount"]);
    assert_eq!(actual["projection_curve"], input["projection"]);
    assert_eq!(actual["model"]["key"], input["model"]["key"]);
    if input["model"]["kind"] == "swaptions" {
        let model = if actual["model"]["origin"]["ParameterScenario"].is_null() {
            &actual["model"]
        } else {
            &actual["model"]["origin"]["ParameterScenario"]["original"]
        };
        let fit = &model["origin"]["SwaptionCalibration"];
        assert_eq!(fit["quotes"], input["model"]["quotes"]);
        assert_eq!(fit["valuation_date"], input["model"]["valuation_date"]);
        assert_eq!(fit["discount_curve"], input["model"]["discount"]);
        assert_eq!(
            fit["fixed_mean_reversion"],
            input["model"]["mean_reversion"]
        );
        close(&model["parameters"]["volatility"], &json!(0.012), 2e-7);
        for (row, quote) in fit["residuals"]
            .as_array()
            .unwrap()
            .iter()
            .zip(input["model"]["quotes"].as_array().unwrap())
        {
            assert_eq!(row["id"], quote["id"]);
            assert!(number(&row["native_residual"]).abs() <= number(&quote["native_tolerance"]));
        }
    }
    assert_eq!(
        actual["deterministic_projection_basis"],
        !input["projection"].is_null()
    );
    let known = expected["known"].as_array().unwrap();
    let fixings = actual["fixings"].as_array().unwrap();
    assert_eq!(known.len(), fixings.len());
    for (a, b) in fixings.iter().zip(known) {
        assert_eq!(a["origin"]["Published"], *b);
    }
    let futures = expected["future"].as_array().unwrap();
    let requirements = actual["future_reset_requirements"].as_array().unwrap();
    assert_eq!(futures.len(), requirements.len());
    for (a, b) in requirements.iter().zip(futures) {
        assert_eq!(a["observation_date"], *b);
    }
    assert_eq!(actual["exercises"].as_array().unwrap().len(), 1);
    let ex = &actual["exercises"][0];
    assert_eq!(ex["right"], expected["exercise"]["right"]);
    for field in ["notice", "exercise", "settlement"] {
        assert_eq!(ex["dates"][field], expected["exercise"][field]);
    }
    assert!(number(&ex["exercise_nodes"]) <= number(&ex["decision_nodes"]));
    let grids = actual["grids"].as_array().unwrap();
    assert_eq!(grids.len(), 4);
    for (i, g) in grids.iter().enumerate() {
        assert_eq!(number(&g["desired_steps"]), (128 * (1 << i)) as f64);
        assert!(number(&g["dirty_price"]).is_finite());
        if i >= 2 {
            assert!(
                number(&g["successive_difference"])
                    <= 0.004 + 1e-8 * number(&g["dirty_price"]).abs()
            );
        }
    }
}
pub fn check(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(out["ok"], "false", "{}: {out}", case["id"]);
        assert_eq!(
            out["error"]["code"], case["expected_error"],
            "{}: {out}",
            case["id"]
        );
        assert!(out.get("result").is_none());
        return;
    }
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    if case["request"]["operation"]["kind"] != "graph_hedge_risk" {
        check_direct(case, &out["result"]["data"]);
        return;
    }
    let data = &out["result"]["data"];
    let op = &case["request"]["operation"];
    let expected = answer(case);
    let vector = &data["vector"];
    let input = &op["input"];
    assert_eq!(data["input"], *input);
    assert_eq!(vector["reference_position"], input["reference_position"]);
    for field in ["id", "instrument", "economics", "calculation", "entity"] {
        assert_eq!(vector[field], input["identity"][field]);
    }
    assert_eq!(
        vector["settlement"],
        op["product"]["input"]["context"]["settlement"]
            .as_str()
            .unwrap_or("2025-10-01")
    );
    assert!(vector["fx"].is_null());
    assert_eq!(vector["credit_scope"], json!([]));
    assert_eq!(
        number(&data["reserved_factor_evaluations"]),
        5. * input["factors"].as_array().unwrap().len() as f64
    );
    let optional = op["product"]["kind"] == "optional_bond";
    for (i, (s, c)) in vector["sensitivities"]
        .as_array()
        .unwrap()
        .iter()
        .zip(expected["factors"].as_array().unwrap())
        .enumerate()
    {
        assert_eq!(s["key"], input["factors"][i]["key"]);
        assert_eq!(s["settings"], input["factors"][i]["settings"]);
        assert_eq!(s["position"], input["reference_position"]);
        assert_eq!(s["snapshot_id"], input["snapshot_id"]);
        for (j, field) in [
            "base_pv",
            "plus_pv",
            "minus_pv",
            "half_plus_pv",
            "half_minus_pv",
        ]
        .iter()
        .enumerate()
        {
            close(
                &s[*field],
                &c["prices"][j],
                if optional { 0.006 } else { 1e-7 },
            );
        }
        let bound = if !optional {
            1e-7
        } else {
            match c["kind"].as_str().unwrap() {
                "a" => 1.,
                "sigma" => 2.,
                _ => 0.0001,
            }
        };
        close(&s["value"], &c["value"], bound);
        close(&s["half_value"], &c["half_value"], bound);
        if optional {
            let cert = &data["optional_factors"][i];
            assert_eq!(cert["sensitivity"], *s);
            let original = &op["product"]["input"];
            for (native, indexes) in [("native", [0, 1, 2]), ("half", [0, 3, 4])] {
                let risk = &cert[native];
                let bump = number(&risk["bump"]);
                for (field, j) in ["base", "up", "down"].iter().zip(indexes) {
                    price(&risk[*field], &c["valuations"][j], original);
                    if c["kind"] == "a" || c["kind"] == "sigma" {
                        if *field != "base" {
                            let scenario = &risk[*field]["model"]["origin"]["ParameterScenario"];
                            assert_eq!(scenario["original"], risk["base"]["model"]);
                            close(
                                &scenario["delta"],
                                &json!(if *field == "up" { bump } else { -bump }),
                                0.,
                            );
                        }
                    } else {
                        assert_eq!(risk[*field]["model"], risk["base"]["model"]);
                    }
                    assert_eq!(risk[*field]["fixings"], risk["base"]["fixings"]);
                    assert_eq!(
                        risk[*field]["future_reset_requirements"],
                        risk["base"]["future_reset_requirements"]
                    );
                }
                assert_eq!(risk["grids"].as_array().unwrap().len(), 4);
                if c["kind"] == "a" || c["kind"] == "sigma" {
                    let last = risk["grids"].as_array().unwrap().last().unwrap();
                    let pv = ["base", "up", "down"].map(|f| number(&risk[f]["dirty_price"]));
                    close(&last["first"], &json!((pv[1] - pv[2]) / (2. * bump)), 1e-12);
                    close(
                        &last["second"],
                        &json!((pv[1] + pv[2] - 2. * pv[0]) / (bump * bump)),
                        1e-9,
                    );
                    let grids = risk["grids"].as_array().unwrap();
                    let settings = &risk["settings"];
                    for pair in grids[grids.len() - 3..].windows(2) {
                        for (measure, absolute, relative) in [
                            (
                                "first",
                                "absolute_first_tolerance",
                                "relative_first_tolerance",
                            ),
                            (
                                "second",
                                "absolute_second_tolerance",
                                "relative_second_tolerance",
                            ),
                        ] {
                            assert!(
                                (number(&pair[1][measure]) - number(&pair[0][measure])).abs()
                                    <= number(&settings[absolute])
                                        + number(&settings[relative])
                                            * number(&pair[1][measure]).abs()
                            );
                        }
                    }
                } else {
                    assert!(bump >= number(&risk["minimum_resolved_bump"]));
                    let pv = ["base", "up", "down"].map(|f| number(&risk[f]["dirty_price"]));
                    close(
                        &risk["duration"],
                        &json!((pv[2] - pv[1]) / (2. * pv[0] * bump)),
                        1e-12,
                    );
                    close(
                        &risk["convexity"],
                        &json!((pv[2] + pv[1] - 2. * pv[0]) / (pv[0] * bump * bump)),
                        1e-10,
                    );
                }
            }
        }
    }
    if optional {
        price(
            &data["valuation"]["Optional"],
            &expected["factors"][0]["valuations"][0],
            &op["product"]["input"],
        );
        assert_eq!(
            number(&data["native_price_evaluations"]),
            6. * input["factors"].as_array().unwrap().len() as f64 + 1.
        );
    }
}
fn check_direct(case: &Value, data: &Value) {
    let op = &case["request"]["operation"];
    if op["kind"] == "calibration" {
        let input = &op["action"]["product"]["input"];
        let c = &case["financial"];
        for (native, indexes) in [("native", [0, 1, 2]), ("half", [0, 3, 4])] {
            for (field, j) in ["base", "up", "down"].iter().zip(indexes) {
                price(&data[native][*field], &c["valuations"][j], input);
            }
        }
        close(
            &data["sensitivity"]["value"],
            &c["value"],
            if c["kind"] == "sigma" { 2. } else { 0.0001 },
        );
        close(
            &data["sensitivity"]["half_value"],
            &c["half_value"],
            if c["kind"] == "sigma" { 2. } else { 0.0001 },
        );
    } else if op["action"]["kind"] == "model_scenario" {
        price(data, &case["financial"], &op["input"]);
    } else {
        assert_eq!(data["parameter"], op["action"]["parameter"]);
        assert_eq!(data["settings"], op["action"]["settings"]);
        for (i, field) in ["base", "up", "down"].iter().enumerate() {
            price(&data[*field], &case["financial"][i], &op["input"]);
        }
    }
}
pub fn owners(case: &Value, output: &str) -> Vec<Value> {
    if !case["expected_error"].is_null()
        || case["request"]["operation"]["product"]["kind"] != "optional_bond"
    {
        return vec![];
    }
    let op = &case["request"]["operation"];
    let out: Value = serde_json::from_str(output).unwrap();
    let mut result = Vec::new();
    for (i, cert) in out["result"]["data"]["optional_factors"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        for name in ["native", "half"] {
            let c = &cert[name];
            let action = if c.get("parameter").is_some() {
                json!({"kind":"model_risk","oas_decimal":op["product"]["oas_decimal"],"parameter":c["parameter"],"bump":c["bump"],"settings":c["settings"]})
            } else {
                json!({"kind":"risk","oas_decimal":op["product"]["oas_decimal"],"axis":c["axis"],"bump":c["bump"],"settings":c["settings"]})
            };
            result.push(json!({"id":format!("{}.owner.{i}.{name}",case["id"].as_str().unwrap()),"expected_error":null,"request":{"schema_version":1,"inputs":case["request"]["inputs"],"operation":{"kind":"optional_bond","input":op["product"]["input"],"action":action}},"owner_expected":c}));
        }
    }
    result
}
pub fn check_owner(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    assert_eq!(out["result"]["data"], case["owner_expected"]);
}
pub fn books(outputs: &[Value]) -> Vec<Value> {
    let templates: Value =
        serde_json::from_str(include_str!("optional-graph-hedge-policy-book.json")).unwrap();
    let mut cases = Vec::new();
    for template in templates["cases"].as_array().unwrap() {
        let mut problem = template["problem"].clone();
        for field in ["holdings", "candidates"] {
            for row in problem[field].as_array_mut().unwrap() {
                let data = &outputs.iter().find(|r| r["id"] == row["column"]).unwrap()
                    ["expected_envelope"]["result"]["data"];
                row["risk"] = data["vector"].clone();
                row["risk"]["fx"] = template["fx"].clone();
                row.as_object_mut().unwrap().remove("column");
            }
        }
        for kind in ["aggregate", "solve"] {
            cases.push(json!({"id":format!("{}.{}",template["id"].as_str().unwrap(),kind),"expected_error":null,"request":{"schema_version":1,"inputs":{"calendars":[],"curves":[],"calibration":null,"helper_calendars":[],"fixings":[],"publications":[]},"operation":{"kind":"hedge","problem":problem,"action":{"kind":kind}}},"expected":template["expected"]}));
        }
        let base = cases.last().unwrap().clone();
        for (label, error) in [
            ("missing_holding", "missing_input"),
            ("missing_factor", "missing_input"),
            ("snapshot", "invalid_input"),
            ("oas_credit_scope", "invalid_input"),
            ("fractional_future", "invalid_input"),
        ] {
            let mut case = base.clone();
            case["id"] = json!(format!(
                "{}.invalid.{label}",
                template["id"].as_str().unwrap()
            ));
            case["expected_error"] = json!(error);
            let p = &mut case["request"]["operation"]["problem"];
            match label {
                "missing_holding" => {
                    p["holdings"].as_array_mut().unwrap().pop();
                }
                "missing_factor" => {
                    p["holdings"][0]["risk"]["sensitivities"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
                "snapshot" => {
                    p["holdings"][0]["risk"]["sensitivities"][0]["snapshot_id"] =
                        json!("OTHER.SNAPSHOT")
                }
                "oas_credit_scope" => {
                    p["factors"][3]["credit"] = json!({"entity_id":"OTHER","seniority":"SeniorUnsecured","document":{"id":"CDS.Standard.Supplied.Confirmation","version":1},"restructuring_clause":{"id":"Supplied.NoRestructuring","version":1},"recovery_fraction":0.4})
                }
                "fractional_future" => p["holdings"][1]["current_quantity"] = json!("1.5"),
                _ => unreachable!(),
            }
            cases.push(case);
        }
        if !template["fx"].is_null() {
            let mut c = base.clone();
            c["id"] = json!("optional_graph.book.invalid.missing_fx");
            c["expected_error"] = json!("missing_input");
            c["request"]["operation"]["problem"]["holdings"][0]["risk"]["fx"] = Value::Null;
            cases.push(c);
        }
    }
    cases
}
pub fn check_book(case: &Value, output: &str) {
    let out: Value = serde_json::from_str(output).unwrap();
    if !case["expected_error"].is_null() {
        assert_eq!(out["ok"], "false", "{}: {out}", case["id"]);
        assert_eq!(
            out["error"]["code"], case["expected_error"],
            "{}: {out}",
            case["id"]
        );
        return;
    }
    assert_eq!(out["ok"], "true", "{}: {out}", case["id"]);
    let d = &out["result"]["data"];
    let exposure = if d.get("exposure").is_some() {
        &d["exposure"]
    } else {
        d
    };
    assert_eq!(exposure["problem"], case["request"]["operation"]["problem"]);
    for field in ["current", "pending", "before"] {
        assert_eq!(exposure[field].as_array().unwrap().len(), 6);
        for (i, (a, b)) in exposure[field]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["expected"][field].as_array().unwrap())
            .enumerate()
        {
            let ratio = if field == "current" {
                3.
            } else if field == "pending" {
                0.5
            } else {
                3.5
            };
            close(
                a,
                b,
                ratio
                    * if i == 4 {
                        1.
                    } else if i == 5 {
                        2.
                    } else {
                        0.0001
                    }
                    + 1e-6,
            );
        }
    }
    if case["request"]["operation"]["action"]["kind"] == "solve" {
        assert_eq!(d["status"], "feasible", "{}: {d}", case["id"]);
        assert_eq!(d["authority"], "proposal_only");
        assert_eq!(number(&d["rank"]["rank"]), 2.);
        assert!(!d["proposal"].is_null());
        let p = &d["proposal"];
        for (actual, expected) in [("quantities", "quantities"), ("final_holdings", "holdings")] {
            for (a, b) in p[actual]
                .as_array()
                .unwrap()
                .iter()
                .zip(case["expected"][expected].as_array().unwrap())
            {
                close(a, b, 1e-5);
            }
        }
        for a in p["after"].as_array().unwrap() {
            close(a, &json!(0.), 1e-7);
        }
        close(&p["total_cost"], &case["expected"]["cost"], 1e-8);
        assert!(p["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["satisfied"] == true));
    }
}
pub fn run(mut execute: impl FnMut(&str) -> String) -> Vec<Value> {
    let mut outputs = Vec::new();
    for case in cases() {
        let out = execute(&request(&case));
        if std::panic::catch_unwind(|| check(&case, &out)).is_err() {
            panic!("complete financial control failed: {}", case["id"]);
        }
        outputs.push(json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&out).unwrap()}));
        for owner in owners(&case, &out) {
            let out = execute(&request(&owner));
            check_owner(&owner, &out);
            outputs.push(json!({"id":owner["id"],"request":owner["request"],"expected_error":null,"expected_envelope":serde_json::from_str::<Value>(&out).unwrap()}));
        }
    }
    for case in books(&outputs) {
        let out = execute(&request(&case));
        check_book(&case, &out);
        outputs.push(json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&out).unwrap()}));
    }
    outputs
}
