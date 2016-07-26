extern crate iron;
use iron::prelude::*;
use iron::status;
use iron::headers::ContentType;
use iron::modifiers::Header;

extern crate router;
use router::Router;

extern crate url;
use url::{Url, Host};

extern crate persistent;
use persistent::{State, Read, Write};
use iron::typemap::Key;

extern crate rustc_serialize;
use rustc_serialize::json;

use std::str::FromStr;
use std::str;

extern crate mustache;
use mustache::{MapBuilder, Template};

use response;
use pies;
use pie_state;
use cache;

pub fn hello_world(req: &mut Request) -> IronResult<Response> {
    response::text("Hello, World!".to_string())
}

pub fn pies(req: &mut Request) -> IronResult<Response> {

    let id_index = req.get::<Read<cache::IdIndex>>().unwrap();
    let redis = req.get::<Read<cache::Redis>>().unwrap();

    let mut pies = vec![];
    let mut bytes = vec![];
    let mut ids = vec![];

    for (id, tuple) in id_index.iter() {
        let show_pie = pies::ShowPie {
            id: tuple.0.id.clone(),
            name: tuple.0.name.clone(),
            image_url: tuple.0.image_url.clone(),
            price_per_slice: tuple.0.price_per_slice.clone(),
            remaining_slices: 0,
            purchases: vec![]
        };
        ids.push(&tuple.0.id);
        pies.push(show_pie);
    }

    let all_remaining = pie_state::get_all_remaining(&redis, &ids);

    for (remaining, pie) in all_remaining.iter().zip(pies.iter_mut()) {
        pie.remaining_slices = remaining.clone();
    }

    pie_template().render(&mut bytes, &pies::ShowPies { pies: pies }).unwrap();
    response::html(format!("<html>{}</html>", str::from_utf8(&bytes).unwrap()))
}

fn pie_template() -> mustache::Template {
    mustache::compile_str("
    {{#pies}}
    <h1><a href=\"/pies/{{id}}\">{{name}}</a></h1>
    <img src=\"{{image_url}}\" width=\"50%\"></img>
    <p>price: {{price_per_slice}}</p>
    <p>remaining: {{remaining_slices}}</p>
    <p> {{#purchases}}
        <p>{{username}} purchased {{slices}}<p>
        {{/purchases}}
    </p>
    {{/pies}}
    ")
}

pub fn pie(req: &mut Request) -> IronResult<Response> {

    let id_index = req.get::<Read<cache::IdIndex>>().unwrap();
    let redis = req.get::<Read<cache::Redis>>().unwrap();
    let url_end = req.url.path.last();

    // req.extensions must go last because borrow checker is dumb
    let pie_option = req.extensions.get::<Router>()
        .unwrap()
        .find("pie_id");

    // return if we can't find pie_id
    let pie_id = if let Some(x) = pie_option {
        u64::from_str(x.trim_right_matches(".json")).unwrap()
    } else {
        return response::not_found()
    };

    // return if we can't find pie in cache
    let (pie, bitvec_pos) = if let Some(x) = id_index.get(&pie_id) {
        x.clone()
    } else {
        return response::not_found()
    };

    let remaining = pie_state::get_remaining(&redis, &pie);

    let show_pie = pies::ShowPie {
        id: pie.id.clone(),
        name: pie.name.clone(),
        image_url: pie.image_url.clone(),
        price_per_slice: pie.price_per_slice.clone(),
        remaining_slices: remaining,
        purchases: pie_state::pie_purchases(&redis, &pie)
    };

    match url_end {
        Some(x) if x.ends_with("json") => {
            let data: String = json::encode(&show_pie).unwrap();
            response::json(data)
        },
        Some(x) => {
            let mut bytes = vec![];
            let mut pies = vec![];
            pies.push(show_pie);
            pie_template().render(&mut bytes, &pies::ShowPies { pies: pies }).unwrap();

            response::html(format!("<html>{}</html>", str::from_utf8(&bytes).unwrap()))
        },
        _ => response::not_found()
    }
}

pub fn purchase(req: &mut Request) -> IronResult<Response> {
    let id_index = req.get::<Read<cache::IdIndex>>().unwrap();
    let redis = req.get::<Read<cache::Redis>>().unwrap();

    let extensions = req.extensions.get::<Router>()
        .unwrap();

    // return if we can't find pie_id
    let pie_id = u64::from_str(extensions.find("pie_id").unwrap()).unwrap();

    let (pie, bitvec_pos) = if let Some(x) = id_index.get(&pie_id) {
        x.clone()
    } else {
        return response::not_found()
    };

    let mut username = None;
    let mut amount = None;
    let mut slices = Some(1);

    let url = req.url.clone().into_generic_url();
    match url.query_pairs() {
        Some(vec) => {
            for &(ref name, ref value) in vec.iter() {
                match name.trim() {
                    "username" => {
                        username = Some(value.clone());
                    },
                    "amount" => {
                        amount = f64::from_str(&value.clone()).ok();
                    },
                    "slices" => {
                        slices = i64::from_str(&value.clone()).ok();
                    }
                    _ => {}
                }
            }
        },
        None => return response::error()
    };

    match (username, amount, slices) {
        (Some(u), Some(a), Some(s)) => {
            let price = pie.price_per_slice * s as f64;

            if (price - a).abs() > 1e-5 {
                response::bad_math()
            } else {
                match pie_state::purchase_pie(&redis, &pie, bitvec_pos, &u, s as isize) {
                    pie_state::PurchaseStatus::Success => {
                        response::purchased()

                    }
                    pie_state::PurchaseStatus::Fatty => {
                        response::glutton()

                    }
                    pie_state::PurchaseStatus::Gone => {
                        response::gone()

                    }
                }
            }
        },
        (Some(u), None, _) => {
            response::bad_math()
        },
        (_, _, _) => {
            response::error()
        }
    }

}

pub fn recommend(req: &mut Request) -> IronResult<Response> {
    let redis = req.get::<Read<cache::Redis>>().unwrap();

    let label_bitvecs = req.get::<Read<cache::LabelBitVec>>().unwrap();
    let sorted_pies = req.get::<Read<cache::SortedPies>>().unwrap();

    let url = req.url.clone().into_generic_url();

    let mut labels = vec![];

    let mut username = None;
    let mut budget = None;

    match url.query_pairs() {
        Some(vec) => {
            for &(ref name, ref value) in vec.iter() {
                match name.trim() {
                    "username" => {
                        username = Some(value.clone());
                    },
                    "budget" => {
                        budget = Some(value.clone());
                    },
                    "labels" => {
                        for label in value.split(",") {
                            labels.push(String::from(label));
                        }
                    }
                    _ => {}
                }
            }
        },
        None => return response::error()
    };

    match (username, budget) {
        (Some(u), Some(b)) => {
            if labels.len() > 0 {
                let pie = pie_state::recommend(
                    &redis,
                    &labels,
                    &sorted_pies,
                    &label_bitvecs,
                    &u,
                    &b
                );

            } else {
                return response::error()
            }
        }
        (_, _) => {
            return response::error()
        }
    }

    response::text(String::from("hello"))
}