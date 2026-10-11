// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::collections::{HashMap, HashSet};

use criterion::{Criterion, criterion_group, criterion_main};
use indexmap::IndexMap;
use nautilus_core::correctness::{
    check_equal, check_in_range_inclusive_u8, check_in_range_inclusive_u64, check_key_in_map,
    check_key_not_in_map, check_map_empty, check_map_not_empty, check_member_in_set,
    check_member_not_in_set, check_predicate_false, check_predicate_true, check_string_contains,
    check_valid_string_ascii, check_valid_string_ascii_optional,
};
use std::hint::black_box;

fn bench_check_predicate_true() {
    black_box(check_predicate_true(true, "predicate must be true")).unwrap();
}

fn bench_check_predicate_false() {
    black_box(check_predicate_false(false, "predicate must be false")).unwrap();
}

fn bench_check_valid_string_ascii() {
    black_box(check_valid_string_ascii("Hello", "param")).unwrap();
}

fn bench_check_valid_string_ascii_optional() {
    black_box(check_valid_string_ascii_optional(Some("Hello"), "param")).unwrap();
}

fn bench_check_string_contains() {
    black_box(check_string_contains("Hello, world!", "world", "param")).unwrap();
}

fn bench_check_equal() {
    black_box(check_equal(&42, &42, "lhs", "rhs")).unwrap();
}

fn bench_check_in_range_inclusive_u8() {
    black_box(check_in_range_inclusive_u8(5, 0, 10, "param")).unwrap();
}

fn bench_check_in_range_inclusive_u64() {
    black_box(check_in_range_inclusive_u64(500, 0, 1000, "param")).unwrap();
}

fn bench_check_map_empty() {
    let empty_map: HashMap<u32, u32> = HashMap::new();
    black_box(check_map_empty(&empty_map, "param")).unwrap();
}

fn bench_check_map_not_empty() {
    let map: HashMap<u32, u32> = HashMap::from([(1, 42)]);
    black_box(check_map_not_empty(&map, "param")).unwrap();
}

fn bench_check_key_in_map() {
    let map: HashMap<u32, u32> = HashMap::from([(1, 42)]);
    black_box(check_key_in_map(&1, &map, "key", "map")).unwrap();
}

fn bench_check_key_not_in_map() {
    let map: HashMap<u32, u32> = HashMap::from([(1, 42)]);
    black_box(check_key_not_in_map(&2, &map, "key", "map")).unwrap();
}

fn bench_check_index_map_in() {
    let map: IndexMap<u32, u32> = IndexMap::from([(1, 42)]);
    black_box(check_key_in_map(&1, &map, "key", "map")).unwrap();
}

fn bench_check_index_map_not_in() {
    let map: IndexMap<u32, u32> = IndexMap::from([(1, 42)]);
    black_box(check_key_not_in_map(&2, &map, "key", "map")).unwrap();
}

fn bench_check_member_in_set() {
    let set: HashSet<u32> = HashSet::from([1, 42]);
    black_box(check_member_in_set(&1, &set, "key", "set")).unwrap();
}

fn bench_check_member_not_in_set() {
    let set: HashSet<u32> = HashSet::from([1, 42]);
    black_box(check_member_not_in_set(&100, &set, "key", "set")).unwrap();
}

// These historical filenames now measure elapsed time, not Valgrind instructions.
fn benchmarks(c: &mut Criterion) {
    c.bench_function("bench_check_predicate_true", |b| {
        b.iter(bench_check_predicate_true);
    });
    c.bench_function("bench_check_predicate_false", |b| {
        b.iter(bench_check_predicate_false);
    });
    c.bench_function("bench_check_valid_string_ascii", |b| {
        b.iter(bench_check_valid_string_ascii);
    });
    c.bench_function("bench_check_valid_string_ascii_optional", |b| {
        b.iter(bench_check_valid_string_ascii_optional);
    });
    c.bench_function("bench_check_string_contains", |b| {
        b.iter(bench_check_string_contains);
    });
    c.bench_function("bench_check_equal", |b| {
        b.iter(bench_check_equal);
    });
    c.bench_function("bench_check_in_range_inclusive_u8", |b| {
        b.iter(bench_check_in_range_inclusive_u8);
    });
    c.bench_function("bench_check_in_range_inclusive_u64", |b| {
        b.iter(bench_check_in_range_inclusive_u64);
    });
    c.bench_function("bench_check_map_empty", |b| {
        b.iter(bench_check_map_empty);
    });
    c.bench_function("bench_check_map_not_empty", |b| {
        b.iter(bench_check_map_not_empty);
    });
    c.bench_function("bench_check_key_in_map", |b| {
        b.iter(bench_check_key_in_map);
    });
    c.bench_function("bench_check_key_not_in_map", |b| {
        b.iter(bench_check_key_not_in_map);
    });
    c.bench_function("bench_check_index_map_in", |b| {
        b.iter(bench_check_index_map_in);
    });
    c.bench_function("bench_check_index_map_not_in", |b| {
        b.iter(bench_check_index_map_not_in);
    });
    c.bench_function("bench_check_member_in_set", |b| {
        b.iter(bench_check_member_in_set);
    });
    c.bench_function("bench_check_member_not_in_set", |b| {
        b.iter(bench_check_member_not_in_set);
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
