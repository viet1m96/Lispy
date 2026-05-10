; Static Pascal string test.

(begin
  (setq s :string "Hello from static pstr")
  (print-str s)
  (print-str "\n")

  (print-str "String literals still work")
  (print-str "\n")

  (print 42)
  (print-str "\n")

  (halt))
