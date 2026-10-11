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

use std::hash::{DefaultHasher, Hash, Hasher};

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::StackStr;
use std::hint::black_box;

fn bench_stackstr_new_short() {
    black_box(StackStr::new("BINANCE"));
}

fn bench_stackstr_new_medium() {
    black_box(StackStr::new("O-20231215-001-001"));
}

fn bench_stackstr_new_max() {
    black_box(StackStr::new("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")); // 36 chars
}

fn bench_stackstr_eq_same() {
    let a = StackStr::new("O-20231215-001-001");
    let b = StackStr::new("O-20231215-001-001");
    black_box(a == b);
}

fn bench_stackstr_eq_different() {
    let a = StackStr::new("O-20231215-001-001");
    let b = StackStr::new("O-20231215-001-002");
    black_box(a == b);
}

fn bench_stackstr_hash() {
    let s = StackStr::new("O-20231215-001-001");
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    black_box(hasher.finish());
}

fn bench_stackstr_as_str() {
    let s = StackStr::new("O-20231215-001-001");
    black_box(s.as_str());
}

fn bench_stackstr_clone() {
    let s = StackStr::new("O-20231215-001-001");
    black_box(s);
}

fn bench_stackstr_from_bytes() {
    black_box(StackStr::from_bytes(b"O-20231215-001-001")).unwrap();
}

fn bench_stackstr_cmp() {
    let a = StackStr::new("AAA-001");
    let b = StackStr::new("ZZZ-999");
    black_box(a.cmp(&b));
}

// These historical filenames now measure elapsed time, not Valgrind instructions.
fn benchmarks(c: &mut Criterion) {
    c.bench_function("bench_stackstr_new_short", |b| {
        b.iter(bench_stackstr_new_short);
    });
    c.bench_function("bench_stackstr_new_medium", |b| {
        b.iter(bench_stackstr_new_medium);
    });
    c.bench_function("bench_stackstr_new_max", |b| {
        b.iter(bench_stackstr_new_max);
    });
    c.bench_function("bench_stackstr_eq_same", |b| {
        b.iter(bench_stackstr_eq_same);
    });
    c.bench_function("bench_stackstr_eq_different", |b| {
        b.iter(bench_stackstr_eq_different);
    });
    c.bench_function("bench_stackstr_hash", |b| {
        b.iter(bench_stackstr_hash);
    });
    c.bench_function("bench_stackstr_as_str", |b| {
        b.iter(bench_stackstr_as_str);
    });
    c.bench_function("bench_stackstr_clone", |b| {
        b.iter(bench_stackstr_clone);
    });
    c.bench_function("bench_stackstr_from_bytes", |b| {
        b.iter(bench_stackstr_from_bytes);
    });
    c.bench_function("bench_stackstr_cmp", |b| {
        b.iter(bench_stackstr_cmp);
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
