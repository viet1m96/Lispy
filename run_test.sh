#!/usr/bin/env bash

for x in {1..8}; do
  echo "Running for 0$x ..."
  cargo run -- compile-lisp examples/0${x}/0${x}*.lisp 0${x}.bin
  cargo run -- run-lisp examples/0${x}/0${x}*.lisp 100000 > examples/0${x}/0${x}.txt
  mv 0${x}.bin examples/0${x}
  mv 0${x}.lst examples/0${x}
done


cargo run -- compile-lisp examples/prob1/prob1.lisp prob1.bin
mv prob1.bin examples/prob1
mv prob1.lst examples/prob1
