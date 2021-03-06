
extern crate r2d2;
extern crate r2d2_redis;
extern crate redis;

use redis::Commands;

use std::collections::HashMap;
use std::ops::Deref;

extern crate bit_vec;
use bit_vec::BitVec;

use pies;

macro_rules! remaining_key { ($x:expr) => (format!("pie-{}-remaining", $x)) }
macro_rules! purchases_key { ($x:expr) => (format!("pie-{}-purchases", $x)) }
macro_rules! user_blacklist_key { ($x:expr) => (format!("user-{}-blacklist", $x)) }
macro_rules! sold_out_key { () => ("pies-sold-out") }

pub fn set_remaining(pool: &r2d2::Pool<r2d2_redis::RedisConnectionManager>, pie: &pies::Pie) {
    let conn = pool.get().expect("redis connection failed");
    let _ : () = conn.set(remaining_key!(pie.id), pie.slices).unwrap();

//    let n : u64 = conn.get(remaining_key!(pie.id)).unwrap();
//    println!("setting remaining for pie {} to {}", pie.name, n);
}

pub fn get_remaining(pool: &r2d2::Pool<r2d2_redis::RedisConnectionManager>, pie: &pies::Pie) -> u64 {
    let conn = pool.get().expect("redis connection failed");
    let n : u64 = conn.get(remaining_key!(pie.id)).unwrap();
    n
}

pub fn get_all_remaining(pool: &r2d2::Pool<r2d2_redis::RedisConnectionManager>, ids: &Vec<&u64>) -> Vec<u64> {
    let conn = pool.get().expect("redis connection failed");
    let keys : Vec<String> = ids.iter().map( |&id|
        remaining_key!(id)
    ).collect();
    let n : Vec<u64> = conn.get(keys).unwrap();
    n
}

pub enum PurchaseStatus {
    Fatty,
    Gone,
    Success
}

const ALLOWED_PIES: isize = 3;

fn get_user_blacklist(conn: &r2d2::PooledConnection<r2d2_redis::RedisConnectionManager>, user: &String) -> BitVec {
    let bits : Vec<u8> = conn.get(user_blacklist_key!(user)).unwrap();
    let bitvec = BitVec::from_bytes(&bits);
//    println!("{}: {:?}", user, bitvec);
    bitvec
}

fn get_pie_soldout(conn: &r2d2::PooledConnection<r2d2_redis::RedisConnectionManager>) -> BitVec {
    let bits : Vec<u8> = conn.get(sold_out_key!()).unwrap();
    let bitvec = BitVec::from_bytes(&bits);
    bitvec
}

fn set_user_blacklist(conn: &r2d2::PooledConnection<r2d2_redis::RedisConnectionManager>, user: &String, bitvec_pos: usize) {
//    println!("{:?} {:?}", user, bitvec_pos);

    // this doesn't work for some reason, so using the raw command version
    // let bitset : bool = conn.setbit(, bitvec_pos, true).unwrap();
    let _ : () = redis::cmd("SETBIT")
        .arg(user_blacklist_key!(user))
        .arg(bitvec_pos)
        .arg(1)
        .query(conn.deref())
        .unwrap();

}

fn set_pie_soldout(conn: &r2d2::PooledConnection<r2d2_redis::RedisConnectionManager>, bitvec_pos: usize) {
    // this doesn't work for some reason, using the raw command version
    // let _ : () = conn.setbit(sold_out_key!(), bitvec_pos, true).unwrap();

    let _ : () = redis::cmd("SETBIT")
        .arg(sold_out_key!())
        .arg(bitvec_pos)
        .arg(1)
        .query(conn.deref())
        .unwrap();
}

fn check_user_blacklist(conn: &r2d2::PooledConnection<r2d2_redis::RedisConnectionManager>, user: &String, bitvec_pos: usize) -> bool {
    let bitset : bool = conn.getbit(user_blacklist_key!(user), bitvec_pos).unwrap();
    bitset
}

fn flatten_bv(labels: &Vec<String>, label_bitvecs: &HashMap<String, BitVec>) -> BitVec {
    let mut bitvecvec = vec![];

    for label in labels {
        match label_bitvecs.get(label) {
            Some(bv) => {
                bitvecvec.push(bv);
            }
            None => {
                return BitVec::from_elem(1, false);
            }
        }
    }

    let mut scratch_bv = match bitvecvec.first() {
        Some(bv) => bv.clone().to_owned(),
        None => {
            return BitVec::from_elem(1, false)
        }
    };

    for bitvec in &bitvecvec {
        scratch_bv.intersect(bitvec);
    }

    scratch_bv
}

fn pad_shorter_bv(bv1: &mut BitVec, bv2: &mut BitVec) -> () {
    let (longer, shorter) = if bv1.len() > bv2.len() {
        (bv1, bv2)
    } else {
        (bv2, bv1)
    };

    let diff = longer.len() - shorter.len();
    shorter.grow(diff, false);
}

pub fn recommend<'pie>(pool: &r2d2::Pool<r2d2_redis::RedisConnectionManager>,
                 labels: &Vec<String>,
                 pies: &'pie Vec<pies::Pie>,
                 label_bitvecs: &HashMap<String, BitVec>,
                 user: &String,
                 budget: &String) -> Option<&'pie pies::Pie> {

    let mut possible_pies = flatten_bv(&labels, &label_bitvecs);
//    println!("possible pies {:?}", possible_pies);

    if possible_pies.none() {
        return None;
    }

    let conn = pool.get().expect("redis connection failed");
    let mut user_blacklist = get_user_blacklist(&conn, user);
    let mut sold_out_pies = get_pie_soldout(&conn);

    pad_shorter_bv(&mut possible_pies, &mut user_blacklist);
    pad_shorter_bv(&mut possible_pies, &mut sold_out_pies);
    pad_shorter_bv(&mut user_blacklist, &mut sold_out_pies);

    // todo: deconfusion comment
    user_blacklist.negate();
    sold_out_pies.negate();
    possible_pies.intersect(&user_blacklist);
    possible_pies.intersect(&sold_out_pies);

//    println!("matching: {:?} ", possible_pies);

    if budget == "cheap" {
        for (i, pie_match) in possible_pies.iter().enumerate().rev() {
//            println!("cheap checking {} -> {}", i, pie_match);
            if pie_match {
                return pies.get(i);
            }
        }
    } else if budget == "premium" {
        for (i, pie_match) in possible_pies.iter().enumerate() {
//            println!("premium checking {} -> {}", i, pie_match);
            if pie_match {
                return pies.get(i);
            }
        }
    } else {
        return None;
    }
    None
}

pub fn purchase_pie(pool: &r2d2::Pool<r2d2_redis::RedisConnectionManager>,
                    pie: &pies::Pie,
                    bitvec_pos: usize,
                    user: &String,
                    amount: isize) -> PurchaseStatus {
    if amount > ALLOWED_PIES {
        return PurchaseStatus::Fatty;
    }

//    println!("bitvec pos for purchase {}", bitvec_pos);

    let conn = pool.get().expect("redis connection failed");
    if check_user_blacklist(&conn, user, bitvec_pos) {
//        println!("blocked purchase via blacklist");
        return PurchaseStatus::Fatty;
    }

    let prev_purchase : bool = conn.hexists(purchases_key!(pie.id), user).unwrap();
    let num_left : isize = conn.get(remaining_key!(pie.id)).unwrap();

    if num_left <= 0 {
        return PurchaseStatus::Gone;
    }

    if amount > num_left {
        return PurchaseStatus::Gone;
    }

    if prev_purchase {
        let previous_amount : isize = conn.hget(purchases_key!(pie.id), user).unwrap();
//        println!("previous amount {:?}", previous_amount);
        if previous_amount + amount > ALLOWED_PIES {
            return PurchaseStatus::Fatty;
        } else {
            if previous_amount + amount == ALLOWED_PIES {
//                println!("reached max amount");
                set_user_blacklist(&conn, user, bitvec_pos)
            }

            let _ : isize = conn.hincr(purchases_key!(pie.id), user, amount).unwrap();
            let _ : () = conn.incr(remaining_key!(pie.id), -1 * amount).unwrap();
//            println!("bought {} pies total!", n)
        }
    } else {
//        println!("buying pie!");
        if amount == ALLOWED_PIES {
            set_user_blacklist(&conn, user, bitvec_pos)
        }

        let _ : isize = conn.hincr(purchases_key!(pie.id), user, amount).unwrap();
        let _ : () = conn.incr(remaining_key!(pie.id), -1 * amount).unwrap();
    }

    if num_left - amount <= 0 {
        set_pie_soldout(&conn, bitvec_pos);
    }

    PurchaseStatus::Success
}

pub fn pie_purchases(pool: &r2d2::Pool<r2d2_redis::RedisConnectionManager>, pie: &pies::Pie) -> Vec<pies::Purchase> {
    let conn = pool.get().expect("redis connection failed");

    let purchases : HashMap<String, u64> = conn.hgetall(purchases_key!(pie.id)).unwrap();

    let mut vec = Vec::new();
    for (user, amount) in &purchases {
        let purchase = pies::Purchase {
            username: user.clone(),
            slices: amount.clone()
        };
        vec.push(purchase);
    }

//    println!("{:?}", vec);
    vec
}
