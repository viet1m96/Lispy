; Bubble sort static digits stored as an array.
; No string indexing/mutation is used.

(begin
  (setq a :array (array 5))

  (array-set a 0 4)
  (array-set a 1 3)
  (array-set a 2 1)
  (array-set a 3 5)
  (array-set a 4 2)

  (setq n :int (array-size a))
  (setq i :int 0)

  (loop while (< i n) do
    (setq j :int 0)

    (loop while (< j (- n 1)) do
      (if (> (array-get a j) (array-get a (+ j 1)))
          (let ((tmp :int (array-get a j)))
            (begin
              (array-set a j (array-get a (+ j 1)))
              (array-set a (+ j 1) tmp)))
          0)

      (setq j :int (+ j 1))
    finally 0)

    (setq i :int (+ i 1))
  finally 0)

  (print (array-get a 0))
  (print (array-get a 1))
  (print (array-get a 2))
  (print (array-get a 3))
  (print (array-get a 4))
  (print-str "\n")

  (halt))
