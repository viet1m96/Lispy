; 64-bit arithmetic and explicit cast

(begin
  (setq a :int 2147483647)
  (setq one64 :i64              1)
  (print (+ (as-i64 a) one64))
  (print-str "\n")

  (setq b :i64 3000000000)
  (setq c :i64 4000000000)
  (print (+ b c))
  (print-str "\n")

  (setq x :i64 65536)
  (setq y :i64 65536)
  (print (* x y))
  (halt))
