; Math with 64 bits numbers

(begin
  (print (+ (i64 2147483647) 1))
  (print-str "\n")
  (print (+ (i64 3000000000) (i64 4000000000)))
  (print-str "\n")
  (print (* (i64 65536) (i64 65536)))
  (halt))
