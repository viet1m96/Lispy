(defun print-array4 ((arr :array)) :string
  (begin
    (print (array-get arr 0))
    (print-str " ")
    (print (array-get arr 1))
    (print-str " ")
    (print (array-get arr 2))
    (print-str " ")
    (print (array-get arr 3))
    (print-str "\n")))

(begin
  (setq a :array (array 4))
  (setq b :array (array 4))
  (setq r :array (array 4))

  (array-set a 0 8)
  (array-set a 1 12)
  (array-set a 2 18)
  (array-set a 3 24)

  (array-set b 0 2)
  (array-set b 1 12)
  (array-set b 2 3)
  (array-set b 3 24)

  (vadd r a b)
  (print-array4 r)

  (vsub r a b)
  (print-array4 r)

  (vmul r a b)
  (print-array4 r)

  (vdiv r a b)
  (print-array4 r)

  (vcmp r a b)
  (print-array4 r)

  (halt))
